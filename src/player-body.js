import { loadClips } from "./anim.js";
import { PlayerCombat } from "./combat.js";
import { ShieldBubble } from "./droideka.js";
import { zeroMaterial } from "./materials.js";
import { modelGroup, modelPack } from "./models.js";
import { motionVectors } from "./motion-vectors.js";
import { SoldierPresentation, HeadlessSoldierPresentation } from "./soldier-presentation.js";
import { buildBody, textureOverrides } from "./units.js";
import { Arsenal } from "./weapons.js";

// The player's bodies, one per class: built the first time he plays the
// class and kept for his later lives, hidden while he plays another
// (docs/human-spawn.md). Each is a bot's body (units.js buildBody) plus what
// only the player's has: weapons aimed from the camera with the HUD
// (PlayerCombat), the first-person arms, a droideka's shield bubble and
// dust, and a jet trooper's flames. The game adopts one at a time
// (game.js adopt()); his Unit stays the same object throughout.
//
// Loading: the classes' models come through the bots' factory (one copy
// each); the arms rigs are fetched here. The class played at load can be
// seeded with the data the game already fetched.

// The droideka preview's cycle (DroidekaElement plays its own states): its
// modes and how long each holds (0: until its clip ends).
const DROIDEKA_CYCLE = [[0, 2.5], [1, 0], [0, 1.5], [2, 0], [0, 2.5], [11, 0], [12, 1.2], [13, 0]];
// The soldier preview's aim point, in its own frame: straight ahead.
const PREVIEW_AIM = [0, 10, 4000];
const FIDGET = [3, 8]; // s between idle fidgets

export class PlayerBodies {
  // c: {
  //   sides         name -> { dir, units, items, texture(name), firePoints }
  //   factory       the bots' UnitFactory (loads and keeps each class's model)
  //   json, bin     fetchers; headless: no drawing (no arms, bubble, flames)
  //   graphics, scene, collision, animMeta, animBuf, maxFlyHeight
  //   ordnance, lights, weaponModels, glowModel(name), anyTexture(name), hud
  //   effects, settings, water, mapName, camera (drawn), simulationCamera
  //   jetEffect(name, side)  a JetEffect's definition
  //   fpAnims, fpAnimBuf     the first-person clips
  //   ports: { FirstPersonView, DROIDEKA_OFFSET, JetpackEffects, JetpackNozzles, DroidekaTrail }
  // }
  constructor(c) {
    this.c = c;
    this.bundles = new Map(); // className -> bundle
    this.seeded = new Map(); // className -> { body, walker }
    this.rigs = new Map(); // rig path -> Promise of { rig, buf, indexBuf } or null
    this.rigData = new Map(); // rig path -> resolved
    this.sideNames = new Map(); // className -> side name, or null
  }

  sideOf(className) {
    if (!this.sideNames.has(className)) {
      let found = null;
      for (const [name, side] of Object.entries(this.c.sides)) if (side.units[className]) found ??= name;
      this.sideNames.set(className, found);
    }
    return this.sideNames.get(className);
  }

  rigPath(className) {
    const sideName = this.sideOf(className);
    const def = sideName && this.c.sides[sideName].units[className];
    return def?.firstPerson ? `${this.c.sides[sideName].dir}/firstperson/${def.firstPerson}` : null;
  }

  // The class played at load, from what the game fetched itself: its model
  // ({ def, buf, indexBuf }), a droideka's clips ({ meta, buf }), and its
  // arms rig ({ rig, buf, indexBuf }).
  seed(className, { body, walker = null, rig = null }) {
    this.seeded.set(className, { body, walker });
    const path = this.rigPath(className);
    if (path && rig) {
      this.rigData.set(path, rig);
      this.rigs.set(path, Promise.resolve(rig));
    }
  }

  // Load what the class needs before he can take it.
  async prepare(className) {
    const c = this.c;
    if (!this.seeded.has(className)) await c.factory.prepare(className);
    const path = this.rigPath(className);
    if (c.headless || !path || this.rigs.has(path)) return this.rigs.get(path);
    const optional = (load) => load.then((x) => x, () => null);
    const ok = (response) => (response.ok ? response : Promise.reject(new Error(response.statusText)));
    this.rigs.set(path, optional(fetch(`${path}.json`).then(ok).then((r) => r.json())).then(async (rig) => {
      if (!rig) return null;
      const [buf, indexBuf] = await Promise.all([c.bin(`${path}.bin`), c.bin(`${path}_index.bin`)]);
      const value = { rig, buf, indexBuf };
      this.rigData.set(path, value);
      return value;
    }));
    return this.rigs.get(path);
  }

  // Start loading the textures the class's body, arms and weapons will show
  // (prepare() first), so his first life as it isn't drawn untextured: with
  // the AI off no bot of the class has loaded them.
  preloadTextures(className) {
    const c = this.c;
    const data = !c.headless && this.data(className);
    if (!data) return;
    const side = c.sides[this.sideOf(className)];
    const def = side.units[className];
    const overrides = textureOverrides(def, data.body.def);
    for (const part of data.body.def.parts) side.texture(overrides[part.texture] || part.texture) || side.texture(part.texture);
    for (const part of this.rigData.get(this.rigPath(className))?.rig.parts || []) c.anyTexture(part.texture);
    for (const weapon of def.weapons || []) {
      for (const name of [weapon.GeometryName, weapon.HighResGeometry]) {
        for (const part of (name && c.weaponModels.get(name)) || []) c.anyTexture(part.texture);
      }
    }
  }

  data(className) {
    const seeded = this.seeded.get(className);
    if (seeded) return seeded;
    const body = this.c.factory?.bodyData(className);
    return body ? { body, walker: this.c.factory.walkerData(className) } : null;
  }

  // Whether a body of the class can be built now.
  ready(className) {
    return Boolean(this.sideOf(className) && this.data(className));
  }

  // The class's body, built the first time; null if it can't be built yet.
  get(className) {
    if (!this.bundles.has(className)) {
      if (!this.ready(className)) return null;
      this.bundles.set(className, this.build(className));
    }
    return this.bundles.get(className);
  }

  all() {
    return [...this.bundles.values()];
  }

  // The body of a class, hidden: everything the player's soldier is made of,
  // built as the game built its one body before it could change class.
  build(className) {
    const c = this.c;
    const sideName = this.sideOf(className);
    const side = c.sides[sideName];
    const def = side.units[className];
    const data = this.data(className);
    const actorDef = data.body.def;
    const { actor, soldier, animator, walker, clips } = buildBody({
      def, data: data.body, walkerData: data.walker, texture: side.texture, graphics: c.graphics, collision: c.collision,
      clips: loadClips(c.animMeta, c.animBuf, actorDef.bones.map((bone) => bone.name)), animMeta: c.animMeta,
      maxFlyHeight: c.maxFlyHeight,
    });
    c.scene.add(actor.root);
    actor.root.visible = false;
    const presentation = new (c.headless ? HeadlessSoldierPresentation : SoldierPresentation)(soldier);
    const combat = new PlayerCombat({
      soldier, def, actor, animator,
      collision: c.collision,
      camera: c.simulationCamera,
      ordnance: c.ordnance,
      lights: c.lights,
      weaponModels: c.weaponModels,
      firePoints: side.firePoints || {},
      flashModel: c.glowModel("muzzleflash"),
      texture: c.anyTexture,
      hud: c.hud,
      caster: (object) => c.graphics.caster(object),
    });
    combat.hudCamera = c.camera;
    combat.deferMuzzles = walker;
    // A droideka: its guns and shield, its bubble, and the dust it raises
    // rolling.
    let bubble = null;
    let droidekaTrail = null;
    if (walker) {
      soldier.arm(def.weapons || []);
      if (!c.headless) {
        const shieldWeapon = (def.weapons || []).find((w) => w.base === "shield");
        const look = c.effects.shields[shieldWeapon?.ShieldEffect];
        bubble = look ? new ShieldBubble(c.scene, look, c.anyTexture, c.settings) : null;
        droidekaTrail = new c.ports.DroidekaTrail({ scene: c.scene, collision: c.collision, water: c.water, settings: c.settings, mapName: c.mapName });
      }
    }
    // The first-person arms, holding the first-person weapons.
    const rig = this.rigData.get(this.rigPath(className));
    const { FirstPersonView, DROIDEKA_OFFSET, JetpackEffects, JetpackNozzles } = c.ports;
    const firstPersonView = !c.headless && rig && c.fpAnims
      ? new FirstPersonView({
          rig: rig.rig,
          parts: modelPack({ [rig.rig.model]: rig.rig.parts }, rig.buf, rig.indexBuf),
          clips: loadClips(c.fpAnims, c.fpAnimBuf, rig.rig.bones.map((bone) => bone.name)),
          texture: c.anyTexture,
          weaponModels: c.weaponModels,
          firePoints: side.firePoints || {},
          flashModel: c.glowModel("muzzleflash"),
          collision: c.collision,
          set: walker ? "droideka" : null,
          offset: walker ? DROIDEKA_OFFSET : null,
        })
      : null;
    // The jetpack's flames (the class's JetEffect), hung from the ribcage.
    const ribcage = actor.byName.bone_ribcage;
    const jetDefinition = !c.headless && def.JetEffect ? c.jetEffect(def.JetEffect, sideName) : [];
    const jetpackEffects = !c.headless && jetDefinition.some(e => /^JetExhaust(?:_Left|_Right)?$/i.test(e.name)) && ribcage ? new JetpackEffects({
      scene: c.scene, settings: c.settings, lights: c.lights, definition: jetDefinition,
    }) : null;
    const jetpackNozzles = actor.jetpackNozzles || (jetpackEffects ? new JetpackNozzles(actor, actorDef) : null);
    return {
      className, side: sideName, sideData: side, def, actorDef, actor, clips, soldier, animator, walker, presentation, combat,
      healthType: (def.HealthType || "person").toLowerCase(),
      bubble, droidekaTrail, jetpackEffects, jetpackNozzles, ribcage, firstPersonView,
    };
  }

  // A body of the class for the spawn screen, outside the world: standing
  // idle with its primary weapon in hand, aimed at (0, 10, 4000) in its own
  // frame (straight ahead, facing +z), with an idle fidget every 3 to 8 s; a
  // droideka cycles its own states instead. Its shadow and motion aren't the
  // world's. { object, update(dt), dispose(), materials, walker } or null.
  preview(className) {
    const c = this.c;
    const sideName = this.sideOf(className);
    const data = sideName && this.data(className);
    if (!data) return null;
    const side = c.sides[sideName];
    const def = side.units[className];
    const graphics = { hiddenLayer: c.graphics.hiddenLayer ?? 1, caster() {} };
    const { actor, soldier, animator, walker, clips } = buildBody({
      def, data: data.body, walkerData: data.walker, texture: side.texture, graphics, collision: c.collision,
      clips: loadClips(c.animMeta, c.animBuf, data.body.def.bones.map((bone) => bone.name)), animMeta: c.animMeta,
    });
    actor.shadow.visible = false;
    const [ax, ay, az] = PREVIEW_AIM;
    soldier.aimYaw = Math.atan2(ax, az);
    soldier.aimPitch = Math.atan2(ay, Math.hypot(ax, az));
    soldier.velocity.set(0, 0, 0);
    let update;
    if (walker) {
      let step = 0;
      let held = 0;
      soldier.enter(DROIDEKA_CYCLE[0][0]);
      animator.reset(soldier.aimYaw);
      update = (dt) => {
        const [mode, hold] = DROIDEKA_CYCLE[step];
        soldier.time += dt;
        held += dt;
        if (mode === 12) soldier.roll += dt * 6;
        const length = soldier.length(mode);
        if (hold ? held >= hold : soldier.time >= length) {
          step = (step + 1) % DROIDEKA_CYCLE.length;
          held = 0;
          soldier.enter(DROIDEKA_CYCLE[step][0]);
        } else if (soldier.time >= length && mode !== 12) {
          soldier.time -= length;
        }
        animator.update(dt, soldier);
        actor.root.updateMatrixWorld(true);
      };
    } else {
      // The primary weapon in the hand, in its animation bank.
      const primary = new Arsenal(def.weapons || [], { player: true }).current(0);
      const parts = primary && c.weaponModels.get(primary.def.GeometryName);
      if (parts && actor.hand) {
        actor.hand.add(modelGroup(parts, (part) => {
          const material = zeroMaterial("dynamic", { map: c.anyTexture(part.texture) });
          actor.materials.push(material);
          return material;
        }));
      }
      animator.setBank(primary?.bank || "rifle");
      animator.reset(soldier.aimYaw);
      const emote = clips.get("human_rifle_stand_idle_emote");
      let fidget = FIDGET[0] + Math.random() * (FIDGET[1] - FIDGET[0]);
      update = (dt) => {
        fidget -= dt;
        if (fidget <= 0 && emote && animator.action === "loco") {
          animator.start("fidget", emote);
          fidget = FIDGET[0] + Math.random() * (FIDGET[1] - FIDGET[0]);
        }
        if (animator.action === "fidget" && animator.actionTime >= emote.duration) animator.action = "loco";
        animator.update(dt, soldier);
        actor.root.updateMatrixWorld(true);
      };
    }
    actor.root.visible = true;
    update(0);
    return {
      object: actor.root,
      materials: actor.materials,
      walker,
      update,
      dispose() {
        actor.root.removeFromParent();
        const motion = actor.body.userData.motion;
        if (motion) motionVectors.release(motion);
        for (const material of actor.materials) material.dispose();
      },
    };
  }
}
