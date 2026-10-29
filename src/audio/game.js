import * as THREE from "three";
import { SoundData, soundRef } from "./data.js";
import { AudioEngine } from "./engine.js";
import { listenSounds } from "./events.js";
import { Acoustics } from "./acoustics.js";
import { Ambience } from "./ambience.js";
import { EnhancedAssets } from "./enhanced.js";
import { LoudnessWindow } from "./mixer.js";
import { AudioPanel } from "./panel.js";
import { BattleMusic } from "./music.js";
import { Chatter } from "./chatter.js";
import { PlayerSounds } from "./player.js";
import { SceneSounds } from "./scene.js";
import { SoldierSounds } from "./soldiers.js";
import { Streams } from "./streams.js";
import { VehicleSounds } from "./vehicles.js";
import { VoiceOver } from "./vo.js";
import { quietWeapon } from "./weapons.js";
import { hashKey } from "./hash.js";

// The battle's sound: who makes which sound when, and where, as the 2004
// game plays it (docs/formats.md, "Sound"; docs/audio.md for the design).
//
// Gameplay code reports discrete events through events.js (a shot, an
// impact, a bounce, an explosion, a hit on the player or another soldier, a
// station serving, a turret firing, a power-up taken) and the AI's chatter
// ("vo" records, through on()); the rest (weapon states, the soldier's
// moves and feet, rounds in flight, command posts, the turret's turning) is
// read off the game's own objects every frame, much as the original's
// entities poll their own state for sound.
//
// Class sound keys come from each class's ODF properties, which
// tools/zero_sound.py writes per class: FireSound, OrdnanceSound,
// SoundProperty and the rest. A value can end in " defer", which only
// delays its lookup in the original.

const AUDIO_ROOT = "/audio";
const GROUND_PROBE = 1.2; // m down from just above the feet
// The pause menu takes the game's own sound (the "ingame" bus) out and puts
// it back this fast.
const PAUSE_FADE = 0.25; // s

export class GameAudio {
  constructor({ settings = null } = {}) {
    this.settings = settings;
    this.engine = new AudioEngine({ settings });
    this.streams = new Streams(this.engine);
    this.engine.streamBusy = (bus) => this.streams.busy(bus);
    this.data = null;
    this.game = null;
    this.ready = false;
    this.missing = new Set();
    this.log = [];
    this.window = new LoudnessWindow();
    this.scratch = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false };
    this.ray = { from: new THREE.Vector3(), down: new THREE.Vector3(0, -1, 0) };
    this.listenerAt = { x: 0, y: 0, z: 0 };
    this.forward = { x: 0, y: 0, z: -1 };
    this.up = { x: 0, y: 1, z: 0 };
    this.unlisten = listenSounds((type, ...args) => this.on(type, ...args));
    settings?.onChange((key) => {
      if (/Volume$/.test(key)) this.engine.applyVolumes();
      else if (key.startsWith("audio")) this.configure();
    });
  }

  // Load the battle's sound: public/audio/index.json maps each mission to
  // the sound sub-level it reads.
  async load(world) {
    const index = await fetch(`${AUDIO_ROOT}/index.json`)
      .then((r) => (r.ok ? r.json() : null))
      .catch(() => null);
    const sublevel = index?.missions?.[world.mission?.name] || index?.maps?.[world.name];
    if (!sublevel) throw new Error(`no sound for ${world.mission?.name || world.name}: run tools/extract.py`);
    const data = await SoundData.load(AUDIO_ROOT, sublevel);
    this.engine.ensureContext();
    await this.engine.renderer.load(AUDIO_ROOT);
    this.engine.enhanced = await EnhancedAssets.open(this.engine.ctx, AUDIO_ROOT, sublevel);
    this.engine.missing = (name) => this.note(`missing ${name}`);
    this.data = data;
    this.engine.setData(data);
    this.streams.setData(data);
    this.configure();
    this.panel = new AudioPanel(this);
    if (new URLSearchParams(location.search).get("audiodebug") === "1") this.panel.toggle(true);
    this.ready = true;
    if (this.game) this.start();
    return data;
  }

  // The game's objects: { camera, soldier, combat, ordnance, controls,
  // posts, battle, collision, teams, actor, water, team, foe, unit, def },
  // and those made later (live, gunner, items, remotes) as they are set.
  attach(game) {
    this.game = game;
    this.acoustics = new Acoustics(game.collision || null);
    if (this.ready) this.start();
  }

  // The player took another body or team between lives. fields: any of the
  // attached objects ({ soldier, combat, actor, unit, def, team, foe }). A
  // new body gets fresh player sounds (the old one's loops stop); a new team
  // its own music and voice-over lines.
  retarget(fields) {
    const g = this.game;
    if (!g) return;
    const soldier = g.soldier;
    const team = g.team;
    Object.assign(g, fields);
    if (!this.started) return;
    if (g.soldier !== soldier) {
      const old = this.player;
      old?.jet?.engine?.stop(0.05);
      old?.engine?.sound?.stop(0.1);
      for (const slot of old?.weapons.values() || []) quietWeapon(slot);
      this.player = new PlayerSounds(this);
    }
    if (g.team !== team) {
      if (this.music) this.music.team = Number(g.team);
      if (this.vo) this.vo.team = Number(g.team);
    }
  }

  start() {
    const g = this.game;
    this.sides = this.teamSides();
    this.player = new PlayerSounds(this);
    this.soldiers = new SoldierSounds(this);
    this.chatter = new Chatter(this);
    this.scene = new SceneSounds(this);
    this.vehicles = new VehicleSounds(this);
    this.ambience = new Ambience(this, this.data.world);
    this.music = new BattleMusic(this, this.data.mission, g.team);
    this.vo = new VoiceOver(this, this.data.mission, this.data.world, g.team, this.sides);
    this.started = true;
  }

  // Each team's side ("Rep", "Cis", "All", "Imp"), from its unit classes.
  teamSides() {
    const out = {};
    for (const [team, t] of Object.entries(this.game.teams || {})) {
      const prefix = (t.classes?.[0]?.[0] || "").split("_")[0].toLowerCase();
      const side = { rep: "Rep", cis: "Cis", all: "All", imp: "Imp" }[prefix];
      if (side) out[Number(team)] = side;
    }
    return out;
  }

  // The audio settings: Original or Enhanced, the output layout, the
  // dynamics profile, mono, and the enhanced mode's clarity stage.
  config() {
    const s = this.settings;
    const get = (key, fallback) => {
      const v = s?.get(key);
      return v === undefined || v === null ? fallback : v;
    };
    return {
      quality: get("audioQuality", "enhanced"),
      output: get("audioOutput", "auto"),
      dynamics: get("audioDynamics", "normal"),
      mono: Boolean(get("audioMono", false)),
      clarity: Boolean(get("audioClarity", true)),
    };
  }

  configure() {
    const r = this.engine.renderer;
    if (!r?.loaded) return;
    r.configure(this.config());
    this.engine.applyVolumes();
    if (r.enhanced) this.engine.enhanced?.preload();
    // The original's reverb is the sub-level's space; the enhanced mode's
    // follows the room around the listener (acoustics.js).
    this.engine.shape = r.enhanced ? (voice) => this.enhancedShape(voice) : (voice) => this.originalShape(voice);
    if (!r.enhanced) r.setRoom(this.originalRoom());
  }

  // The sub-level's sound space and its reverb preset, as EAX-style
  // parameters (all but silent on Theed: its preset's RoomGain and
  // ReverbGain are 0).
  originalRoom() {
    const data = this.data;
    const spaces = data ? [...(data.byKind.get("space")?.values() || [])] : [];
    const space = spaces[spaces.length - 1] || null;
    const preset = space ? data.object("I3DL2ReverbPreset", space.I3DL2ReverbPreset) : null;
    this.spaceSend = space?.RoomGain ?? 0;
    if (!preset) return { room: 0 };
    return {
      room: preset.RoomGain ?? 0,
      roomHF: Math.max(preset.RoomHFGain ?? 1, 0.001),
      decay: preset.DecayTime ?? 1,
      hfRatio: preset.DecayHFRatio ?? 0.5,
      reflections: preset.ERGain ?? 0,
      reflDelay: preset.ERDelay ?? 0.02,
      reverb: preset.ReverbGain ?? 0,
      reverbDelay: preset.ReverbDelay ?? 0.04,
      diffusion: Math.min(Math.max(preset.Diffusion ?? 1, 0), 1),
      density: 1,
      hfReference: preset.HFReference ?? 5000,
    };
  }

  // The enhanced extras: air, occlusion, the room, the LFE, and the
  // loudness window's ducking.
  enhancedShape(voice) {
    const extra = this.acoustics.shape(voice);
    extra.gain *= this.window.gain(voice.category);
    return extra;
  }

  // In the original each voice's reverb send is its space's RoomGain (the
  // listener's space; Theed's is 0), not the sound's own ReverbGain.
  originalShape(voice) {
    const extra = voice.extra;
    extra.send = this.spaceSend ?? 0;
    extra.gain = 1;
    extra.lfe = 0;
    return extra;
  }

  note(text) {
    if (this.missing.has(text)) return;
    this.missing.add(text);
    this.log.push(text);
    if (this.log.length > 200) this.log.shift();
  }

  // A class's sound for a key, or null (the first, for a key set per side);
  // remembered per battle's data.
  classSound(className, key) {
    const data = this.data;
    if (!data) return null;
    if (this.refsFor !== data) {
      this.refsFor = data;
      this.refs = new Map();
    }
    let refs = this.refs.get(className);
    if (!refs) this.refs.set(className, (refs = new Map()));
    let ref = refs.get(key);
    if (ref === undefined) {
      const value = data.classSounds(className)?.[key];
      ref = value ? soundRef([].concat(value)[0]) : null;
      refs.set(key, ref);
    }
    return ref;
  }

  // Play a class's sound for a key; options as AudioEngine.play.
  playClass(className, key, options) {
    const ref = this.classSound(className, key);
    if (!ref) return null;
    return this.engine.play(ref.name, options);
  }

  // A tow cable's attach or release sound, by the weapon's name: the
  // weapon's last word swapped for towcable_<what>, else the snowspeeder's.
  towSound(vehicle, def, what) {
    const stem = String(def?.class || "all_weap_fly_snowspeeder_reargun").replace(/_[a-z0-9]+$/i, "");
    const options = { position: vehicle.position, category: "weapons" };
    return this.engine.play(`${stem}_towcable_${what}`, options) || this.engine.play(`all_weap_fly_snowspeeder_towcable_${what}`, options);
  }

  // What is straight under a point: the hit ({ point, terrain }) or null.
  groundUnder(p) {
    const collision = this.game?.collision;
    if (!collision) return null;
    this.ray.from.set(p.x, p.y + 0.3, p.z);
    return collision.cast(this.ray.from, this.ray.down, GROUND_PROBE, this.hit);
  }

  // The material box (an object with a FoleyFXGroup) a point is in, the
  // smallest if several.
  materialAt(p) {
    let best = null;
    let size = Infinity;
    for (const box of this.data?.world.materials || []) {
      const [a, b] = [box.min, box.max];
      if (p.x < a[0] - 0.3 || p.y < a[1] - 0.3 || p.z < a[2] - 0.3 || p.x > b[0] + 0.3 || p.y > b[1] + 0.3 || p.z > b[2] + 0.3) continue;
      const v = (b[0] - a[0]) * (b[1] - a[1]) * (b[2] - a[2]);
      if (v < size) {
        size = v;
        best = box;
      }
    }
    return best;
  }

  // Discrete events from the game (events.js).
  on(type, ...args) {
    if (!this.ready || !this.started) return;
    const handler = EVENTS[type];
    if (handler) handler(this, ...args);
  }

  // The pause menu (src/pause): the game's own sound fades out while it is
  // up and back to where it was after, as the original's pause takes the
  // ingame bus out and restores it. The engine's clock keeps running, so
  // the menu's own sounds (shellfx, not under ingame) play, repeat and fade;
  // the game's sounds see no time pass.
  setPaused(paused) {
    paused = Boolean(paused);
    if (this.paused === paused) return;
    this.paused = paused;
    const key = hashKey("ingame");
    const bus = this.engine.buses.get(key);
    if (!bus) return;
    if (paused) this.resumeLevel = bus.fade?.to ?? bus.fadeLevel ?? 1;
    this.engine.fadeBus(key, paused ? 0 : this.resumeLevel ?? 1, PAUSE_FADE);
  }

  // The battle is over: every stream and voice stops, and sound effects
  // fade out over a second (the original game's game-over steps).
  endOfBattle() {
    this.streams.stopAll(0.05);
    for (const voice of this.engine.voices) voice.stop?.(0.05);
    this.scene?.stop();
    this.vehicles?.stop();
    this.engine.fadeBus("0xeceb308a", 0, 1);
  }

  update(dt) {
    if (!this.ready || !this.started) return;
    const g = this.game;
    const cam = g.camera;
    cam.updateMatrixWorld();
    const m = cam.matrixWorld.elements;
    const at = this.listenerAt;
    at.x = m[12];
    at.y = m[13];
    at.z = m[14];
    const forward = this.forward;
    forward.x = -m[8];
    forward.y = -m[9];
    forward.z = -m[10];
    const up = this.up;
    up.x = m[4];
    up.y = m[5];
    up.z = m[6];
    this.engine.setListener(this.listenerAt, this.forward, this.up);
    // Paused, the game's sounds see no time pass; the engine's does.
    const game = this.paused ? 0 : dt;
    this.player.update(game);
    this.soldiers.update(game);
    this.scene.update(game);
    this.vehicles.update(game);
    if (this.engine.renderer?.enhanced) {
      this.acoustics?.update(game, this.engine.listener, this.engine.voices, this.engine.renderer);
      this.window.update(game, this.engine.voices);
    }
    this.ambience?.update(game);
    this.music?.update(game);
    this.vo?.update(game);
    this.engine.update(dt);
    this.streams.update(dt);
  }
}

// What each event plays. Handlers take (audio, ...the event's arguments).
const EVENTS = {
  // A round leaves the player's weapon, or a repair tick lands (combat.js).
  fire(audio, combat, shot) {
    audio.player.fire(combat, shot);
  },
  // Another soldier's: a bot's (combat.js), another player's online (his
  // round's launch, network-entity-game.js).
  actorFire(audio, combat, shot) {
    audio.soldiers.fire(combat, shot);
  },
  // Another soldier hurt or killed (combat.js; online, his replicated health
  // and death).
  actorHurt(audio, combat, amount) {
    audio.soldiers.damage(combat, amount, false);
  },
  actorDeath(audio, combat, amount) {
    audio.soldiers.damage(combat, amount, true);
  },
  // The AI's chatter: a "vo" record from the simulation journal (game.js).
  vo(audio, record) {
    audio.chatter.say(record);
  },
  // A turret's gun fires (gunner.js).
  turretFire(audio, turret, shot) {
    audio.scene.turretFire(turret, shot);
  },
  // The snowspeeder's harpoon took hold of a walker's leg, and let go
  // (vehicles.js launchTow, releaseTow): the sound bank's
  // <weapon>_towcable_attach and _release (all_weap_fly_snowspeeder_reargun
  // plays all_weap_fly_snowspeeder_towcable_attach; the 2004 data names
  // them by the bank alone, no ODF key).
  towAttach(audio, vehicle, def) {
    audio.towSound(vehicle, def, "attach");
  },
  towRelease(audio, vehicle, def) {
    audio.towSound(vehicle, def, "release");
  },
  // Ordnance meets the world (ordnance.js): its CollisionSound there.
  impact(audio, def, point) {
    audio.playClass(def.class, "CollisionSound", { position: point, category: "weapons" });
  },
  // A grenade meets the world, bouncing or sticking: CollisionOtherSound.
  bounce(audio, def, point) {
    audio.playClass(def.class, "CollisionOtherSound", { position: point, category: "weapons" });
  },
  // Ordnance into the water: a grenade's CollisionWaterSound.
  splash(audio, def, point) {
    audio.playClass(def.class, "CollisionWaterSound", { position: point, category: "weapons" });
  },
  // An explosion: its SoundProperty.
  explosion(audio, explosion, position) {
    audio.playClass(explosion.class, "SoundProperty", { position, category: "explosions" });
  },
  // The player is hurt or killed (combat.js).
  hurt(audio, combat, amount) {
    audio.player.damage(combat, amount, false);
  },
  death(audio, combat, amount) {
    audio.player.damage(combat, amount, true);
  },
  // A sound by name at a point (props.js: an animated prop's trigger sound
  // and a creature's KillSoldierSound, the sarlacc's belch).
  sound(audio, name, position) { audio.engine.play(name, { position, category: "foley" }); },
  // A power-up taken (items.js).
  pickup(audio, def, position) {
    audio.playClass(def.class, "PowerupSound", { position, category: "foley" });
  },
  // A recharge droid served someone (props.js).
  serve(audio, building) {
    audio.scene.served(building);
  },
  // Command posts (commandposts.js events, from main.js): the capture or
  // loss sound at the post, and the voice-over line for it.
  post(audio, event) {
    const desc = event.post?.desc || {};
    const cls = audio.data.world.posts?.[desc.name]?.class || "com_bldg_controlzone";
    const at = event.post?.position;
    const key = { captured: "CapturedSound", neutralized: "LostSound", lost: "LostSound" }[event.kind];
    if (key) audio.playClass(cls, key, at ? { position: at, category: "foley" } : { category: "foley" });
    audio.vo?.post(event);
  },
  // The player has gone outside the battlefield (src/battlefield.js): his
  // side's voice-over line.
  leaving(audio) {
    audio.vo?.leaving();
  },
  // An interface sound (the spawn screen's), by the mission's SetSoundEffect
  // name (SpawnDisplaySpawnPointChange...), or the sound's own name (the
  // scoreboard's shell_select_change...); 2D, as the scope's are.
  interface(audio, effect) {
    const sound = audio.data.mission.effects?.[effect] || effect;
    if (sound) audio.engine.play(sound, { category: "interface" });
  },
  // A bus, by name, ramped to a level over some seconds (the original game's
  // bus fade: the scoreboard takes "ingame", the game's own sound, out and
  // back; the menu sounds' "shellfx" isn't under it).
  busFade(audio, bus, level, seconds) {
    audio.engine.fadeBus(hashKey(bus), level, seconds);
  },
};
