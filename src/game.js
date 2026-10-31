import { InputLatencyTrace } from './runtime/input-latency-trace.js';
import { WeaponPrediction, predictedPress } from './runtime/weapon-prediction.js';
import {entityPostNames} from './runtime/entity-spawn-posts.js';
import { captureCompactSoldier, restoreCompactSoldier } from './runtime/network-entity-soldier-compact.js';
import { EntitySeats } from './runtime/entity-seats.js';
import { MountedWeaponPrediction } from './runtime/mounted-weapon-prediction.js';
import {predictRemote,RemotePredictionContext} from './runtime/network-remote-prediction.js';
import {captureRemoteReplay,restoreRemoteReplay} from './runtime/network-remote-replay.js';
import { InfantryPrediction } from './runtime/infantry-prediction.js';
import { DroidekaPrediction } from './runtime/droideka-prediction.js';
import { captureDroidekaState, restoreDroidekaState } from './runtime/network-droideka-state.js';
import { retireAuthoritativeInputs } from './runtime/input-retirement.js';
import { PredictionWindow, retainPredictionCommands, resetPredictionIdentity } from './runtime/prediction-window.js';
import { discreteActions, predictionCommands, expireMovement } from './runtime/network-inputs.js';
import { commandBodyAim } from './runtime/command-body-aim.js';
import { PredictedShots } from './runtime/predicted-shots.js';
import { renderedViewTick } from './runtime/view-tick.js';
import { predictVehicle } from './runtime/vehicle-prediction.js';
import { transportPosts } from './flight/spawn.js';
import { FLIGHT_CLASSES } from './flight/config.js';
import { flightBindings } from './flight/controls.js';
import { keyName } from './menus/hotkeys.js';
import { leaveKilled, mountInReach, personLease, seatInput, seatScheme } from './boarding.js';
import { FlightDebug } from './flight/debug.js';
import { lockedUnit, hasMissileLock, targetingWeapon } from "./weapon-targeting.js";
import { createChat } from "./chat.js";
import { followBattle } from "./presence.js";
import { Vehicles, VehicleDriver, VehicleCamera } from "./vehicles.js";
import { VehicleEffects } from "./vehicle-effects.js";
import { TurretSeats } from "./seats.js";
import * as THREE from "three";
import { Debris } from "./debris.js";
import { Destruction } from "./destruction.js";
import { givePowerup, Items } from "./items.js";
import { RemoteDroids, BODY_RADIUS as DROID_RADIUS } from "./remote.js";
import { BLAST_BODY } from "./combat.js";
import { CameraShake } from "./camera-shake.js";
import { resolveOrdnanceOwner } from "./ordnance-owner.js";
import { NetworkRemoteWorld, reconRandom } from "./runtime/network-remote-world.js";
import { Lightning } from "./lightning.js";
import { lighting } from "./materials.js";
import { DeathCamera, SoldierCamera, deadFov } from "./camera.js";
import { DeathShot } from "./death-shot.js";
import { DeathTrace } from "./death-trace.js";
import { CollisionWorld } from "./collision.js";
import { CommandPosts } from "./commandposts.js";
import { Effects } from "./effects.js";
import { Gunner } from "./gunner.js";
import { LiveObjects } from "./props.js";
import { killRegion, registerObjectives } from "./objectives.js";
import { buildRegion, inRegion } from "./regions.js";
import { CrowdShadows } from "./crowd-shadows.js";
import { findReticleTarget, radarRelation } from "./hud-units.js";
import { modelGroup, modelPack, partMaterial } from "./models.js";
import { LightPool, Ordnance } from "./ordnance.js";
import { BATTLE_SIZES, Settings, WORLD_SETTINGS } from "./settings.js";
import { contentLabels } from "./battle-stats.js";
import { Battle } from "./battle.js";
import { makeHit } from "./collision.js";
import { takeImpacts } from "./collision-damage.js";
import { Battlefield, LeavingWarning, loadBoundary } from "./battlefield.js";
import { Units } from "./registry.js";
import { SimEvents } from "./sim-events.js";
import { Unit, UnitFactory } from "./units.js";
import { goneUp } from "./droideka.js";
import { ClassicRandom, RandomStreams } from "./ai/random.js";
import { createAI } from "./ai/index.js";
import { createServiceCandidates } from "./ai/services.js";
import { CommandHistory, CommandControls, queueHumanCommand, stillCommand } from "./runtime/commands.js";
import { InputLead, advanceInputClock, replayTicks, CorrectionSmoother } from "./runtime/reconcile.js";
import { HeadlessGraphics, HeadlessHud, HeadlessAudio, HeadlessControls } from "./runtime/headless-ports.js";
import { WaterState } from "./runtime/water-state.js";
import { droidekaWaterDepth, waterDamageRule, waterDamageStep } from "./vehicle-water.js";
import { spawnPlacedItems } from "./placed-items.js";
import { FixedClock } from "./runtime/fixed-clock.js";
import { PlayoutClock } from "./runtime/playout.js";
import { FramePacer, ResolutionController } from "./runtime/frame-pacing.js";
import { RuntimeProfiler } from "./runtime-profiler.js";
import { attachPostAI, factionOf, friendlySpawn } from "./ai/classic/goals.js";
import { callsFromMapMission, missionTeams, teamTable } from "./ai/classic/spawning.js";
import { Trap, padOnGround, sideOfTeams } from "./traps.js";
import { CORPSE_LINGER, HumanSpawn, RESPAWN_DELAY, choosePost, spawnPointAt, switchHumanTeam, applyAuthoritativeHumanTeam } from "./human-spawn.js";
import { PlayerBodies } from "./player-body.js";
import { playableClasses, spawnPolicy } from "./spawn-screen/policy.js";
import { selectAIMission, selectMission } from "./mission.js";
import { commonModel } from "./interface/common-models.js";
import { nextUrl } from "./launch.js";

// onProgress(step, loaded, total): the load's progress, for the loading
// screen (src/shell/loading.js): "data", "world", "shaders", and
// "textures" with how many of how many have arrived.
export async function createGame({ query = "", headless = false, savedSettings = {}, onProgress = null } = {}) {
const { TextureLibrary } = headless ? {} : await import("./textures.js");
const { DroidekaTrail, JetpackEffects, JetpackNozzles, Foliage, EnvironmentEffects, DROIDEKA_OFFSET, FirstPersonView, Graphics, BattleHud, Controls, HtmlOptionsMenu, createPauseMenu, PageTheme, Water, Weather, Atmosphere, SkyFlyers, Snow, GameAudio, WaterShore, buildWorld } = headless ? {} : await import("./browser-ports.js");
const params = new URLSearchParams(query);
const runtimeProfile = params.get("profileRuntime") === "1" ? new RuntimeProfiler() : null;
const inputTrace = params.get("traceInput") === "1" ? new InputLatencyTrace() : null;
const app = headless ? null : document.querySelector("#app");

const stats = headless ? null : document.querySelector("#stats");

// URL options override the saved settings for this visit.
const overrides = {};
if (params.has("scale")) overrides.renderScale = Number(params.get("scale")) || 1;
if (params.has("sens")) overrides.mouseSensitivity = Number(params.get("sens")) || 5;
if (params.has("invert")) overrides.invertMouse = params.get("invert") === "1";
if (params.has("rate")) overrides.physicsRate = params.get("rate");
if (params.has("shadows")) overrides.shadows = params.get("shadows");
if (params.has("ai")) overrides.aiMode = params.get("ai");
if (params.has("tier")) overrides.aiDifficulty = params.get("tier").toLowerCase();
if (params.has("heroes")) overrides.aiHeroes = params.get("heroes") === "1";
if (params.has("size")) overrides.battleSize = params.get("size");
const settings = new Settings({ ...savedSettings, ...overrides });

const graphics = headless ? new HeadlessGraphics(settings) : new Graphics(app, settings);
// The bots' shadows, bodies and guns, in one draw per cascade.
if (!headless) graphics.crowdShadows = new CrowdShadows(graphics);
const { scene, camera, renderer } = graphics;

// Which map to load. The side and unit the player plays come from the URL
// once the map is known (src/spawn-screen/policy.js).
const MAP = params.get("map") || "nab2";
// The bots: ?ai=classic fields both teams with the Classic AI (src/ai), stub
// with the foundation's stand-in brain; off (the default for now) leaves the
// player alone. ?tier= is the Classic difficulty (easy, medium, hard).
const AI_MODE = settings.get("aiMode");
const SEED = Number(params.get("seed")) || 1;
const TIER = { easy: 1, medium: 2, hard: 3 }[settings.get("aiDifficulty")] || 2;
const HEROES = settings.get("aiHeroes");
const UNIT_SCALE = BATTLE_SIZES[settings.get("battleSize")]?.scale ?? 1;

const KILL_Y = -110;
// The soldier's bounding sphere: drops leave from its centre, and mines feel
// its size.
const BODY_MIDDLE = 1.048; // m above the feet
const BODY_RADIUS = 1.278; // m


async function start() {
  // In the live display loop, catch-up after a hitch is bounded by measured
  // tick cost (35 ms of ticks per frame) and 8 ticks of kept debt. A PC that
  // cannot hold 60 Hz plays the battle slower than real time, still
  // responsive, rather than at a few frames a second; every tick still runs,
  // at full length. Frames driven by tools and replays run every tick.
  const worldClock = new FixedClock({ budget: 0.035, maxDebt: 8 });
  const framePacer = new FramePacer(settings.get("frameLimit"));
  const resolution = new ResolutionController();
  function configurePacing() {
    framePacer.setLimit(settings.get("frameLimit"));
    const target = Number(settings.get("dynamicResolution")) || 0;
    resolution.setTarget(framePacer.limit ? Math.min(target, framePacer.limit) : target);
    graphics.setAdaptiveScale?.(1);
  }
  configurePacing();
  settings.onChange(key => {
    if (key === "frameLimit" || key === "dynamicResolution" || key === "renderScale") configurePacing();
  });
  const streams = new RandomStreams(SEED);
  const gameplayRandom = () => streams.get("gameplay").uniform();
  let vehicles = null, driver = null, gunner = null, turretSeats = null, vehicleEffects = null;
  let humanReticleTarget = null;
  let factory = null, ai = null, randomB = null, networkRemotes = null;
  let markReady = () => {};
  const ready = new Promise((resolve) => {
    markReady = resolve;
  });
  const texLoader = headless ? null : new THREE.TextureLoader(new THREE.LoadingManager(markReady, (url, loaded, total) => onProgress?.("textures", loaded, total), markReady));
  if (headless) markReady();
  // The battle holds still until the loading screen hands over (begin()):
  // no tick runs, here or in a worker, so nothing happens unseen. Nor is it
  // drawn under the screen: its frames would only hold up the screen's own
  // in the GPU process (docs/shell.md). The screen unveils it two frames
  // before it goes, so the first frames' costs fall under its last picture.
  let held = !headless, veiled = !headless;
  // Texture requests still out, and when that last changed: ready resolves
  // the first time the count reaches zero, but bodies and spawn-screen
  // models ask for more after that (loaded() waits for the quiet).
  const texturesOut = new Map(); // url -> requests still out
  let texturesChanged = performance.now();
  if (texLoader) {
    const manager = texLoader.manager, start = manager.itemStart.bind(manager), end = manager.itemEnd.bind(manager);
    manager.itemStart = (url) => { texturesOut.set(url, (texturesOut.get(url) || 0) + 1); texturesChanged = performance.now(); start(url); };
    manager.itemEnd = (url) => {
      const left = (texturesOut.get(url) || 1) - 1;
      if (left > 0) texturesOut.set(url, left); else texturesOut.delete(url);
      texturesChanged = performance.now();
      end(url);
    };
  }
  // A map's own data, the side its soldier comes from, and what every side shares.
  const mapDir = `/maps/${MAP}`;
  const json = (path) => fetch(path).then((response) => response.json());
  const bin = (path) => fetch(path).then((response) => response.arrayBuffer());
  const [mapData, animMeta, common, effectsDef] = await Promise.all([
    json(`${mapDir}/map.json`),
    json("/common/anims.json"),
    json("/common/common.json"),
    json("/common/effects.json"),
  ]);
  // The battle the URL plays of those the map holds (src/mission.js): its
  // mission, spawn screen and vehicles in map.json's places for all below.
  const world = selectMission(mapData, params);
  // Whether the spawn screen runs, and the side and unit played first: the
  // URL's, or the screen's default (src/spawn-screen/policy.js).
  const policy = spawnPolicy(params, world.mission);
  const SIDE = policy.side;
  const UNIT = policy.className;
  const sideDir = `/sides/${SIDE}`;
  const [side, weaponsDef] = await Promise.all([json(`${sideDir}/units.json`), json(`${sideDir}/weapons.json`)]);
  // The unit played: its class, its body (actors/<model>), and its arms.
  let soldierDef = side.units[UNIT];
  if (!soldierDef) throw new Error(`${UNIT} isn't in ${sideDir} (it has ${Object.keys(side.units).join(", ")})`);
  const bodyFile = `${sideDir}/actors/${soldierDef.model}`;
  const [actorDef, geomBuf, indexBuf, actorBuf, actorIndexBuf, animBuf, collBuf, collIndexBuf, collPolyBuf, groundBuf, groundIndexBuf] =
    await Promise.all([
      json(`${bodyFile}.json`),
      headless ? null : bin(`${mapDir}/geom.bin`),
      headless ? null : bin(`${mapDir}/index.bin`),
      bin(`${bodyFile}.bin`),
      bin(`${bodyFile}_index.bin`),
      bin("/common/anims.bin"),
      bin(`${mapDir}/collision.bin`),
      bin(`${mapDir}/collision_index.bin`),
      bin(`${mapDir}/collision_poly.bin`),
      bin(`${mapDir}/terrain_collision.bin`),
      bin(`${mapDir}/terrain_collision_index.bin`),
    ]);
  // The first-person arms rig and its clips, when the side has them.
  const optional = (load) => load.then((x) => x, () => null);
  const ok = (response) => (response.ok ? response : Promise.reject(new Error(response.statusText)));
  // A droideka walks by clips of its own; a side's own effects (its shield).
  let walker = soldierDef.base === "walkerdroid";
  const [walkerMeta, walkerBuf, sideFx] = await Promise.all([
    walker ? json(`${sideDir}/actors/${soldierDef.clips}.json`) : null,
    walker ? bin(`${sideDir}/actors/${soldierDef.clips}.bin`) : null,
    optional(fetch(`${sideDir}/effects.json`).then(ok).then((r) => r.json())),
  ]);
  const [sideFxBuf, sideFxIndexBuf] = await Promise.all([
    sideFx && Object.keys(sideFx.models || {}).length ? bin(`${sideDir}/fxmodels.bin`) : null,
    sideFx && Object.keys(sideFx.models || {}).length ? bin(`${sideDir}/fxmodels_index.bin`) : null,
  ]);
  const armsFile = soldierDef.firstPerson ? `${sideDir}/firstperson/${soldierDef.firstPerson}` : null;
  const [fpRig, fpAnims, propsDef, skyFlyersDef] = await Promise.all([
    !headless && armsFile ? optional(fetch(`${armsFile}.json`).then(ok).then((r) => r.json())) : null,
    headless ? null : optional(fetch("/common/anims_fp.json").then(ok).then((r) => r.json())),
    optional(fetch(`${mapDir}/props.json`).then(ok).then((r) => r.json())),
    // The ships crossing the sky (src/sky-flyers.js, sky/ beside map.json):
    // drawn only, so neither a world worker nor a server reads them, and
    // optional, as a map prepared before them has none.
    headless ? null : optional(fetch(`${mapDir}/sky/flyers.json`).then(ok).then((r) => r.json())),
  ]);
  // The classes this battle draws, if any.
  const skyFlying = Boolean(skyFlyersDef?.missions?.[world.mission?.name]?.length || skyFlyersDef?.enhanced?.[world.mission?.name]?.length);
  // A turret's pieces (and its effects' meshes), drawn only.
  const propsExtras = !headless && Object.keys(propsDef?.extras || {}).length;
  const [propsBuf, propsIndexBuf, propsCollBuf, propsCollIndexBuf, propsCollPolyBuf, propsExtraBuf, propsExtraIndexBuf, propsAnimBuf, skyFlyersBuf, skyFlyersIndexBuf] = await Promise.all([
    propsDef ? bin(`${mapDir}/props.bin`) : null,
    propsDef ? bin(`${mapDir}/props_index.bin`) : null,
    propsDef ? optional(bin(`${mapDir}/props_collision.bin`)) : null,
    propsDef ? optional(bin(`${mapDir}/props_collision_index.bin`)) : null,
    propsDef ? optional(bin(`${mapDir}/props_collision_poly.bin`)) : null,
    propsExtras ? optional(bin(`${mapDir}/props_extras.bin`)) : null,
    propsExtras ? optional(bin(`${mapDir}/props_extras_index.bin`)) : null,
    propsDef?.animations && Object.keys(propsDef.animations).length ? optional(bin(`${mapDir}/props_anims.bin`)) : null,
    skyFlying ? optional(bin(`${mapDir}/sky/flyers.bin`)) : null,
    skyFlying ? optional(bin(`${mapDir}/sky/flyers_index.bin`)) : null,
  ]);
  const [fpBuf, fpIndexBuf, fpAnimBuf] = await Promise.all([
    fpRig ? bin(`${armsFile}.bin`) : null,
    fpRig ? bin(`${armsFile}_index.bin`) : null,
    fpAnims ? bin("/common/anims_fp.bin") : null,
  ]);
  const [holoBuf, holoIndexBuf, lumiBuf, weaponBuf, weaponIndexBuf, fxBuf, fxIndexBuf] = await Promise.all([
    headless ? null : bin("/common/holograms.bin"),
    headless ? null : bin("/common/holograms_index.bin"),
    !headless && world.lumi ? bin(`${mapDir}/${world.lumi.file}`) : null,
    bin(`${sideDir}/weapons.bin`),
    bin(`${sideDir}/weapons_index.bin`),
    bin("/common/fxmodels.bin"),
    bin("/common/fxmodels_index.bin"),
  ]);
  // The battle's vehicle pack, in a folder of its own (vehicles/<mission>/).
  const vehicleDir = `${mapDir}/${String(world.vehicles || "vehicles/").replace(/\/[^/]*$/, "")}`;
  const vehicleDef = world.vehicles ? await fetch(`${mapDir}/${world.vehicles}`).then(r => {
    if (!r.ok) throw new Error(`Vehicle pack unavailable (${r.status}); re-run tools/zero_vehicle.py ${MAP}`);
    return r.json();
  }) : null;
  const [vehicleBuf, vehicleIndexBuf, vehicleExtraBuf, vehicleExtraIndexBuf, vehicleAnimationBuf] = vehicleDef ? await Promise.all([
    bin(`${vehicleDir}/models.bin`), bin(`${vehicleDir}/models_index.bin`),
    bin(`${vehicleDir}/extras.bin`), bin(`${vehicleDir}/extras_index.bin`),
    vehicleDef.animations ? bin(`${vehicleDir}/animations.bin`) : null,
  ]) : [];
  // The mission's other sides, for the bots: their classes, weapons, and
  // effects. A side that wasn't extracted fields no units.
  const otherSides = {};
  await Promise.all([...new Set(world.mission?.sides || [])].filter((name) => name !== SIDE).map(async (name) => {
    const dir = `/sides/${name}`;
    try {
      const [units, weapons, fx] = await Promise.all([
        fetch(`${dir}/units.json`).then(ok).then((r) => r.json()),
        fetch(`${dir}/weapons.json`).then(ok).then((r) => r.json()),
        optional(fetch(`${dir}/effects.json`).then(ok).then((r) => r.json())),
      ]);
      const [weaponBuf, weaponIndexBuf] = await Promise.all([bin(`${dir}/weapons.bin`), bin(`${dir}/weapons_index.bin`)]);
      const fxBufs = fx && Object.keys(fx.models || {}).length ? await Promise.all([bin(`${dir}/fxmodels.bin`), bin(`${dir}/fxmodels_index.bin`)]) : null;
      otherSides[name] = { dir, units, weapons, weaponBuf, weaponIndexBuf, fx, fxBufs };
    } catch (error) {
      console.warn(`side ${name} isn't extracted (${error.message}); its units won't spawn`);
    }
  }));
  graphics.setMap(world);
  onProgress?.("data");
  // The battle's sound (src/audio); the game runs the same without it.
  const audio = headless ? new HeadlessAudio() : new GameAudio({ settings });
  audio.load(world).catch((error) => console.warn(`audio: ${error.message}`));

  // Textures by name, looked up in the archive folder that holds them. They
  // stay as stored (no sRGB decode): the game lit and blended in gamma space.
  // GPU-compressed (KTX2) where the import made them (textures.js).
  const textures = new Map();
  const library = headless ? null : new TextureLibrary({
    renderer: graphics.renderer, manager: texLoader.manager, compressed: settings.get("textureCompression") !== false,
  });
  if (library) {
    const tables = [[mapDir, world.textures], [sideDir, side.textures], ["/common", common.textures], [vehicleDir, vehicleDef?.textures],
      ...Object.values(otherSides).map((other) => [other.dir, other.units.textures]), [`${mapDir}/sky`, skyFlying ? skyFlyersDef.textures : null]];
    await library.prepare(tables.flatMap(([dir, table]) => Object.values(table || {}).map((path) => `${dir}/${path}`)));
  }
  function loadTexture(url) {
    if (headless) return null;
    if (!textures.has(url)) {
      const map = library.load(url);
      map.flipY = false;
      map.wrapS = THREE.RepeatWrapping;
      map.wrapT = THREE.RepeatWrapping;
      textures.set(url, graphics.texture(map));
    }
    return textures.get(url);
  }
  const resolver = (dir, table) => {
    const paths = new Map(Object.entries(table).map(([name, path]) => [name.toLowerCase(), `${dir}/${path}`]));
    return name => { const path = paths.get(name?.toLowerCase()); return path ? loadTexture(path) : null; };
  };
  const mapTexture = resolver(mapDir, world.textures);
  const sideTexture = resolver(sideDir, side.textures);
  const commonTexture = resolver("/common", common.textures);
  const vehicleTexture = resolver(vehicleDir, vehicleDef?.textures || {});
  const otherTextures = Object.values(otherSides).map((other) => resolver(other.dir, other.units.textures || {}));
  const otherTexture = (name) => {
    for (const texture of otherTextures) {
      const found = texture(name);
      if (found) return found;
    }
    return null;
  };
  const anyTexture = (name) => sideTexture(name) || commonTexture(name) || mapTexture(name) || vehicleTexture(name) || otherTexture(name);
  // The HUD's icons by name. The other sides' come first, so a name any
  // other table has keeps its old meaning; they serve the classes the player
  // can switch to.
  const hudPaths = new Map();
  for (const [dir, table] of [...Object.values(otherSides).map((other) => [other.dir, other.units.textures || {}]),
    [mapDir, world.textures], [vehicleDir, vehicleDef?.textures || {}], ['/common', common.textures], [sideDir, side.textures]]) {
    for (const [name, path] of Object.entries(table)) hudPaths.set(name.toLowerCase(), `${dir}/${path}`);
  }
  const iconUrl = name => hudPaths.get(name?.toLowerCase()) || null;
  // Every side's classes, items, textures and weapon fire points: the bots
  // are built from these, and so is any body the player takes.
  const sides = {
    [SIDE]: { dir: sideDir, units: side.units, items: side.items || {}, texture: sideTexture, firePoints: weaponsDef.firePoints || {} },
  };
  for (const [name, other] of Object.entries(otherSides)) {
    sides[name] = { dir: other.dir, units: other.units.units, items: other.units.items || {}, texture: resolver(other.dir, other.units.textures || {}), firePoints: other.weapons.firePoints || {} };
  }

  if (!headless) buildWorld(graphics, world, geomBuf, indexBuf, mapTexture);
  onProgress?.("world");
  // Theed's flower beds and creepers.
  const foliage = !headless && world.foliage ? new Foliage(scene, world.foliage, { texture: mapTexture, nearEnd: () => lighting.fade.value.y, gpu: true }) : null;
  // The terrain's water, where the map has some.
  const water = world.water ? (headless ? new WaterState(world.water) : new Water({ data: world.water, texture: mapTexture, graphics, weather: world.weather })) : null;
  const waterContact = {};
  // A droideka in deep water (the walkerdroid's own rule, src/vehicle-water.js).
  const droidekaWater = (body, health, dt) => {
    const rule = body.waterRule ??= waterDamageRule(body.def) || false;
    const hurt = waterDamageStep(body, dt, water, rule || null, body.position, droidekaWaterDepth(body.def, body.rolled));
    if (hurt > 0) health.damage(hurt);
  };
  const groundPositions = new Float32Array(groundBuf);
  const collision = new CollisionWorld(
    new Float32Array(collBuf),
    new Uint32Array(collIndexBuf),
    new Uint32Array(collPolyBuf),
    groundPositions,
    new Uint32Array(groundIndexBuf),
  );
  // Below the battlefield: KILL_Y, or 10 m under the lowest ground where the
  // ground lies lower (Naboo Plains runs from -95 m down to -140 m, its posts'
  // spawn paths below -110 m).
  let killY = KILL_Y;
  for (let i = 1; i < groundPositions.length; i += 3) killY = Math.min(killY, groundPositions[i] - 10);
  const waterShore = !headless && water ? new WaterShore({ water, collision, graphics }) : null;
  // Precipitation: rain (Type Streaks) or, as on Hoth, snow (Type Quads).
  const snowing = world.weather?.precipitation?.Type?.[0]?.toLowerCase() === "quads";
  const weather = !headless && world.weather?.precipitation && !snowing ? new Weather({data: world.weather, scene, collision, water, settings}) : null;
  const snow = !headless && snowing ? new Snow({ data: world.weather, scene, collision, water, texture: mapTexture }) : null;
  // The map's haze: FogCloud and its placed dust and mist (atmosphere.js).
  const atmosphere = !headless && world.atmosphere ? new Atmosphere({ data: world.atmosphere, scene, collision, texture: mapTexture }) : null;
  // The ships crossing the sky (SkyObject): in the far scene, after the dome
  // and the far terrain, as the original flies them or, with Ocean and sky
  // Enhanced, every class of the battle's era, at any view distance. Each
  // page flies its own; ?seed= repeats them.
  const skyFlyers = skyFlying && skyFlyersBuf && skyFlyersIndexBuf ? new SkyFlyers({
    data: skyFlyersDef, mission: world.mission.name, buffer: skyFlyersBuf, indexBuffer: skyFlyersIndexBuf,
    texture: resolver(`${mapDir}/sky`, skyFlyersDef.textures || {}), scene: graphics.farScene, graphics,
    seed: params.has("seed") ? SEED : (Math.random() * 0x100000000) >>> 0,
  }) : null;
  graphics.localLights.collision = collision;
  const view = new SoldierCamera(camera, collision, { ...soldierDef, cameraMotions: common.cameraMotions }, { responsive: true });
  const simulationCamera = camera.clone();
  const simulationView = new SoldierCamera(simulationCamera, collision, soldierDef, { responsive: true });
  const simulationEye = new THREE.Vector3();
  // The zoom the world's soldier camera last had (1 in a vehicle or turret).
  let simulationZoom = 1;
  // The death camera (src/camera.js): from his death until he has a body
  // again it is the world's camera, the one the Classic AI's scheduling
  // reads, as the original's is the camera everything sees. So it steps in
  // the world tick from the world alone.
  const deathView = new DeathCamera(collision);
  // What the page draws meanwhile (src/death-shot.js): its own camera, from
  // the body and the killer as they are drawn, in every host.
  const deathShot = headless ? null : new DeathShot({
    world: collision,
    refs: {
      get units() { return units; },
      get characters() { return battle.characters; },
      get vehicles() { return vehicles?.vehicles; },
      relation: (team) => units.relation(team, playerTeam),
    },
    trace: new DeathTrace(scene),
  });

  // The battle: the player's team is the one whose unit classes include the
  // unit played; the command posts and the HUD follow from the map. He can
  // change teams between lives (setHumanTeam).
  const teams = world.mission?.teams || {};
  let playerTeam = policy.team;
  let enemyTeam = Number(Object.keys(teams).find((t) => Number(t) !== playerTeam) || 2);
  // The common hologram pack: the posts' side logos, and the spawn screen's
  // logos and room (spawnPort.commonModel). Not loaded headless.
  const holograms = headless ? null : modelPack(common.holograms, holoBuf, holoIndexBuf);
  const posts = new CommandPosts(scene, world, {
    logos: headless ? new Map() : hologramLogos(world.commandPost?.logos || {}, holograms, commonTexture),
    textures: { flare: commonTexture("commandpostflare"), logo: commonTexture("com_icon_controlzone") },
    playerTeam,
    settings,
    headless,
  });
  const hud = headless ? new HeadlessHud() : new BattleHud(graphics.renderer, {
    url: (name) => iconUrl(name) || (world.textures[name] ? `${mapDir}/${world.textures[name]}` : null),
    fonts: common.fonts,
    strings: common.strings,
    map: world,
    mapImage: world.minimap?.texture ? `${mapDir}/${world.minimap.texture}` : null,
  });
  // The key a craft's prompt names (src/hud.js flightPrompt): the flight
  // action's first binding, as the original names keys.
  hud.keyName = (action) => keyName(flightBindings(settings.get("flightBindings"))[action]?.[0] ?? "");
  const battle = new Battle({
    teams,
    posts,
    own: playerTeam,
    foe: enemyTeam,
    onOver: (loser) => hud.announce(loser === enemyTeam ? "Victory" : "Defeat", loser === enemyTeam ? "friendly" : "enemy"),
    relation: (a, b) => units.relation(a, b),
    living: (team) => ai?.spawning?.teams[team]?.living ?? null,
  });

  // Built as the original game builds them (src/regions.js).
  const placed = (region) => ({ ...region, built: buildRegion(region) });
  // The regions this battle's script names with AddDeathRegion (Tatooine:
  // Dune Sea's sarlacc pit, Sarlac01), and those named deathregion*.
  const scripted = new Set((world.mission?.deathRegions || []).map((name) => String(name).toLowerCase()));
  const deathRegions = (world.deathRegions || [])
    .filter((region) => /^deathregion/i.test(region.name || "deathregion") || scripted.has(String(region.name).toLowerCase()))
    .map(placed);
  const lightRegions = (world.lightRegions || []).map(placed);
  const local = new THREE.Vector3();
  const inside = (region, p) => inRegion(region.built, p, local);
  function inDeathRegion(p) {
    return p.y < killY || deathRegions.some((region) => inside(region, p));
  }

  // The soldier's light, as the game sets it each frame from the middle of
  // his body. Light regions ("shadow02 a=1.0 d=0.0") weigh in over a 2.5 m
  // band at their edges: the sun is scaled by the least of 1 - (1 - d) x
  // weight, and an ambient factor a under 1 pulls the ambient toward black
  // (between 1 and 2, toward white). On Bespin shadow02 puts the levels under
  // the city in shade. Maps whose sky asks for it also darken soldiers by the
  // LUMI map, min(LUMI / 255 x 1.2 + 0.3, 1).
  const lumi = world.lumi && lumiBuf ? { ...world.lumi, data: new Uint8Array(lumiBuf) } : null;
  const BAND = 2.5;
  const ramp = (x) => THREE.MathUtils.clamp(x / (2 * BAND), 0, 1);
  function regionWeight(region, p) {
    const { built } = region;
    local.copy(p).applyMatrix4(built.inverse);
    const [, b, c] = built.half;
    const a = built.type === "box" ? built.half[0] : built.radius;
    if (region.type === "sphere") return ramp(a + BAND - local.length());
    if (region.type === "box") {
      const dx = Math.abs(local.x) - a;
      const dy = Math.abs(local.y) - b;
      const dz = Math.abs(local.z) - c;
      if (dx < 0 && dy < 0 && dz < 0) return ramp(Math.min(-dx, -dy, -dz) + BAND);
      return ramp(BAND - Math.hypot(Math.max(dx, 0), Math.max(dy, 0), Math.max(dz, 0)));
    }
    if (region.type === "cylinder") {
      const r = Math.hypot(local.x, local.z);
      const y = Math.abs(local.y);
      if (r > a + BAND || y > b + BAND) return 0;
      const deepR = r <= a - BAND;
      const deepY = y <= b - BAND;
      if (deepR && deepY) return 1;
      // As the original game has it: with only one axis in its band the ramp
      // uses the distance from the axis, not from the surface.
      if (deepY) return ramp(BAND - r);
      if (deepR) return ramp(BAND - y);
      const wr = ramp(a + BAND - r);
      const wy = ramp(b + BAND - y);
      return Math.max(1 - Math.hypot(1 - wr, 1 - wy), 0);
    }
    return 0;
  }
  const bodyCenter = new THREE.Vector3();
  function lumiAt(x, z) {
    const fx = (x - lumi.origin[0]) / lumi.unit;
    const fz = (z - lumi.origin[1]) / lumi.unit;
    const last = lumi.size - 1;
    const x0 = THREE.MathUtils.clamp(Math.floor(fx), 0, last);
    const z0 = THREE.MathUtils.clamp(Math.floor(fz), 0, last);
    const x1 = Math.min(x0 + 1, last);
    const z1 = Math.min(z0 + 1, last);
    const tx = THREE.MathUtils.clamp(fx - x0, 0, 1);
    const tz = THREE.MathUtils.clamp(fz - z0, 0, 1);
    const at = (i, k) => lumi.data[i * lumi.size + k];
    const near = at(x0, z0) + (at(x1, z0) - at(x0, z0)) * tx;
    const far = at(x0, z1) + (at(x1, z1) - at(x0, z1)) * tx;
    return near + (far - near) * tz;
  }
  const soldierLight = { sun: 1, darken: 0, lighten: 0, brightness: 1 };
  const unitLight = { sun: 1, darken: 0, lighten: 0, brightness: 1 };
  function lightSoldier(p) {
    applyLight(actor.materials, lightAt(p, soldierLight));
  }
  // A bot's body, lit the same way.
  function lightActor(body, p) {
    applyLight(body.materials, lightAt(p, unitLight));
  }
  function applyLight(materials, { sun, darken, lighten, brightness }) {
    for (const material of materials) {
      material.uniforms.sunScale.value = sun;
      material.uniforms.brightness.value = brightness;
      material.uniforms.ambientPull.value.set(darken, lighten);
    }
  }
  function lightAt(p, out) {
    bodyCenter.set(p.x, p.y + 0.9, p.z);
    let sun = 1;
    let darken = 0;
    let lighten = 0;
    for (const region of lightRegions) {
      const w = regionWeight(region, bodyCenter);
      if (w <= 0) continue;
      if (region.directional !== undefined) sun = Math.min(sun, 1 - (1 - region.directional) * w);
      const a = region.ambient;
      if (a !== undefined && a < 1) darken = Math.max(darken, (1 - a) * w);
      if (a !== undefined && a > 1 && a < 2) lighten = Math.max(lighten, (a - 1) * w);
    }
    const brightness = lumi ? Math.min((lumiAt(p.x, p.z) / 255) * 1.2 + 0.3, 1) : 1;
    return Object.assign(out, { sun, darken, lighten, brightness });
  }

  // Weapons: what the soldier carries and fires, the effects they set off,
  // and the lights of muzzle flashes, bolts, and blasts.
  const fxModels = modelPack(effectsDef.models, fxBuf, fxIndexBuf);
  if (sideFxBuf) for (const [name, parts] of modelPack(sideFx.models, sideFxBuf, sideFxIndexBuf)) fxModels.set(name, parts);
  for (const other of Object.values(otherSides)) {
    if (!other.fxBufs) continue;
    for (const [name, parts] of modelPack(other.fx.models, ...other.fxBufs)) if (!fxModels.has(name)) fxModels.set(name, parts);
  }
  const vehicleExtras = vehicleDef ? modelPack(vehicleDef.extras, vehicleExtraBuf, vehicleExtraIndexBuf) : new Map();
  for (const [name, parts] of vehicleExtras) if (!fxModels.has(name)) fxModels.set(name, parts);
  const turretExtras = propsExtraBuf && propsExtraIndexBuf ? modelPack(propsDef.extras, propsExtraBuf, propsExtraIndexBuf) : new Map();
  for (const [name, parts] of turretExtras) if (!fxModels.has(name)) fxModels.set(name, parts);
  const glowModel = (name) => {
    const parts = fxModels.get(name);
    if (!parts) return null;
    return () => {
      const material = new THREE.MeshBasicMaterial({
        map: anyTexture(parts[0].texture),
        transparent: true,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        side: THREE.DoubleSide,
      });
      const group = modelGroup(parts, () => material);
      group.userData.material = material;
      return group;
    };
  };
  // The side's own effects (the droideka's) join the shared ones.
  const otherEffects = Object.assign({}, ...Object.values(otherSides).map((other) => other.fx?.effects || {}));
  // The turrets' effects (props.json) fill in what no other library has.
  const effects = new Effects(scene, { ...(propsDef?.effects || {}), ...effectsDef.effects, ...otherEffects, ...(sideFx?.effects || {}), ...(vehicleDef?.effects || {}) },
    { texture: anyTexture, model: glowModel, gpu: !headless });
  if (headless) effects.play = () => null;
  else { effects.preload(); await effects.warmup(renderer, camera); }
  const environment = headless ? null : await EnvironmentEffects.load({ map: MAP, graphics, settings, loadTexture });
  const lights = headless ? { add() {}, flash() {}, update() {} } : new LightPool(scene, 8);
  const weaponModels = modelPack(weaponsDef.models, weaponBuf, weaponIndexBuf);
  for (const other of Object.values(otherSides)) {
    for (const [name, parts] of modelPack(other.weapons.models, other.weaponBuf, other.weaponIndexBuf)) if (!weaponModels.has(name)) weaponModels.set(name, parts);
  }
  for (const [name, parts] of [...vehicleExtras, ...turretExtras]) if (!weaponModels.has(name)) weaponModels.set(name, parts);
  // A hull's or turret's pieces (fx.chunks) start loading now too, so none
  // flies black.
  for (const def of [...Object.values(vehicleDef?.classes || {}), ...(propsDef?.instances || [])]) {
    for (const chunk of def.fx?.chunks || []) for (const part of weaponModels.get(chunk.ChunkGeometryName) || []) anyTexture(part.texture);
  }
  // Start loading what a class will show before it's needed: its HUD icons,
  // its rounds and their glow, its pieces, what it may drop or throw. The
  // class played now; at map start, every class he can switch to.
  function preloadClass(def, items = {}) {
    hud.preload(
      (def.weapons || []).flatMap((w) => [
        w.IconTexture,
        w.ReticuleTexture,
        w.ScopeTexture,
        w.droid?.HealthTexture,
        ...(w.droid?.weapons || []).flatMap((dw) => [dw.IconTexture, dw.ReticuleTexture, dw.ScopeTexture]),
      ]).filter(Boolean),
    );
    for (const weapon of def.weapons || []) {
      for (const name of [weapon.IconTexture, weapon.ReticuleTexture, weapon.ordnance?.LaserTexture, "lightglow"]) {
        if (name) anyTexture(name);
      }
      // A recon droid's weapons and look.
      for (const w of weapon.droid?.weapons || []) {
        for (const name of [w.ordnance?.LaserTexture, w.strike?.LaserTexture]) if (name) anyTexture(name);
      }
    }
    for (const name of [
      ...(def.chunks || []).map((chunk) => chunk.ChunkGeometryName),
      ...(def.drops || []).map((drop) => items[drop.class]?.GeometryName),
      ...(def.weapons || []).flatMap((w) => [w.ordnance, ...(w.charges || []).map((c) => c.ordnance), w.item, w.droid].map((o) => o?.GeometryName)),
    ]) {
      for (const part of (name && weaponModels.get(name)) || []) anyTexture(part.texture);
    }
  }
  preloadClass(soldierDef, side.items || {});
  let combat = null;
  let items = null;
  let live = null;
  // Things in flight: rounds, a droid's pieces, mines and power-ups.
  const flyingMaterials = new WeakMap();
  const flyingModel = (name) => {
    const parts = weaponModels.get(name);
    if (!parts) return null;
    return () => {
      const group = modelGroup(parts, (part) => {
        if (!flyingMaterials.has(part)) flyingMaterials.set(part, partMaterial(part, anyTexture));
        return flyingMaterials.get(part);
      });
      for (const child of group.children) graphics.caster(child);
      group.addEventListener("removed", () => {
        for (const child of group.children) graphics.casters.delete(child);
      });
      return group;
    };
  };
  // Everyone on the field (src/registry.js) and what happens to them
  // (src/sim-events.js), for ordnance, capture, and the bots.
  const units = new Units();
  // Who counts for whom at a post, and how each team looks: the registry's
  // mission table (set below, once the mission's calls are read).
  posts.teams = units;
  const simEvents = new SimEvents();
  // The AI's chatter is the audio's to play (src/audio/chatter.js); a world
  // worker records the call for the page (src/runtime/network-entity-game.js).
  simEvents.listen((record) => record.kind === "vo" && audio.on("vo", record));
  let simTime = 0;
  // Whose side a round or blast is on: a unit's team; the player's for his
  // own recon droid and its strike.
  const teamOf = (owner) => (owner && typeof owner === "object" ? owner.team ?? 0 : owner === "player" ? playerTeam : 0);
  const refOf = (owner) => (owner && typeof owner === "object" && "generation" in owner ? units.ref(owner) : null);
  // Friendly fire, as the original game has it: rounds stop on whatever soldier
  // they meet, and one rule scales the damage of rounds and blasts alike.
  // A hit from the soldier's own team (the same team number; allies of
  // another number take it in full) is scaled by the Friendly Fire option,
  // on (x1) by default and off (x0, which also spares a soldier his own
  // blasts); a bot's hit on a human teammate does nothing at all.
  const isHumanOwner = (owner) => owner === "player" || Boolean(owner?.human);
  function teamDamage(owner, unit) {
    const team = teamOf(owner);
    if (!team || team !== unit.team) return 1;
    if (!isHumanOwner(owner) && unit.human) return 0;
    return settings.get("friendlyFire") === false ? 0 : 1;
  }
  const remoteBlasts = new Map();
  for (const side of Object.values(sides)) for (const unit of Object.values(side.units || {})) {
    for (const weapon of unit.weapons || []) {
      const droid = weapon.droid;
      if (!droid) continue;
      if (droid.explosion) remoteBlasts.set(droid.explosion.class, 0.65);
      for (const gun of droid.weapons || []) if (gun.strike?.ExplosionImpact) remoteBlasts.set(gun.strike.ExplosionImpact.class, 2.5);
    }
  }
  const ordnance = new Ordnance({
    scene,
    collision,
    effects,
    lights,
    texture: anyTexture,
    model: flyingModel,
    onExplode: (explosion, position, owner) => {
      const remoteScale = remoteBlasts.get(explosion.class);
      if (remoteScale) hooks.presented("remoteBlast", position.clone(), remoteScale);
      for (const unit of units.all()) {
        // The living take its damage, push and shake; a body still lying
        // on the field (dead, not yet removed) is thrown by it too. Of the
        // seated, only a turret's shown gunner is hurt (SeatLease.blastable).
        if (!unit.soldier || unit.removed || !(unit.alive || unit.dead) || unit.seat && !unit.seat.blastable || (unit.human && driver?.active)) continue;
        unit.combat.explode(explosion, position, owner, teamDamage(owner, unit));
      }
      simEvents.emit("explosion", { position, team: teamOf(owner), source: refOf(owner), radius: explosion.DamageRadiusOuter || 0 });
      // Hulls and buildings take the same friendly fire factor soldiers do.
      const teamScale = (target) => teamDamage(owner, target);
      vehicles?.blast(explosion, position, owner, teamScale);
      items?.blast(explosion, position);
      live?.blast(explosion, position, owner, teamScale);
      combat?.remotes?.blast(explosion, position);
      networkRemotes?.blast(explosion, position);
    },
    onHit: (body, def, owner, direction) => { const teamScale = (target) => teamDamage(owner, target); live?.struck(body, def, owner, teamScale, direction); vehicles?.struck(body, def, owner, teamScale); },
  });
  // The render camera, for the Enhanced sniper look (sniper-trace.js) to tell
  // the shooter's own eye view from a shot it watches.
  ordnance.viewCamera = headless ? null : camera;
  // And his first-person gun's muzzle as drawn now, which his own shot's
  // trail stays on while the gun kicks and settles.
  ordnance.viewMuzzle = headless ? null : () => {
    const weapon = combat?.firstPerson ? combat.arsenal?.current(0) : null;
    return weapon ? combat.firePoint(weapon)?.position ?? null : null;
  };
  const debris = new Debris({ scene, collision, effects, model: flyingModel });
  // Enhanced droid deaths and wrecks: drawn only, never in a headless world.
  const destruction = headless ? null : new Destruction({ scene, collision, lights, settings, model: flyingModel, lightAt });
  // Presentation the world starts itself: a droid's pieces flying, an
  // Enhanced death, a droideka's death blasts. Drawn here at once; a world
  // worker records them instead (hooks.presented), and the page that shows
  // its battle draws them (src/runtime/network-entity-game.js).
  const droidBlasts = ["explosion", "med_explosion", "small_explosion"];
  function presentEvent(kind, unit, health = 0, source = null, hitKind = null) {
    if (headless || !unit) return;
    if (kind === "remoteBlast") {
      if (destruction?.enhanced) destruction.blast(unit, health);
      return;
    }
    // A droideka's shield taking a hit: its bubble flares (health: the amount).
    if (kind === "shieldHit") { (unit === playerUnit ? bubble : unit.bubble)?.hit(source, health, unit.soldier, hitKind); return; }
    if (kind === "blowApart") {
      unit.actor.root.updateMatrixWorld(true);
      if (!destruction?.blowApart(unit)) debris.blowApart(unit.classDef.chunks || [], unit.actor.byName, unit.position, unit.velocity, health);
      // Gone for the rest of its life, as the world hid it (Unit.goUp).
      unit.goUp?.();
    } else if (kind === "died") destruction?.died(unit);
    else if (kind === "deathBlast") {
      if (destruction?.deathBlast(unit)) return;
      const bone = unit.actor.bones[Math.floor(Math.random() * unit.actor.bones.length)];
      effects.play(droidBlasts[Math.floor(Math.random() * droidBlasts.length)], bone.getWorldPosition(new THREE.Vector3()));
    }
  }
  const hooks = { presented(kind,unit,amount=0,source=null,hitKind=null) {
    presentEvent(kind,unit,amount,source,hitKind);
    if(kind!=='died')simEvents.emit('presentation',{event:{kind,
      ...(kind==='remoteBlast'?{position:unit.toArray()}:{ownerId:unit.id,ownerGeneration:unit.generation}),
      amount,source:source?.toArray?.()??null,hitKind}});
  } };
  function presentEffect(name,position) {
    effects.play(name,position);
    if (name === "headshot" && !headless) ordnance.presentHeadshot(position);
    simEvents.emit('presentation',{event:{kind:'effect',name,position:position.toArray()}});
  }
  // Mines and power-ups lying about.
  items = new Items({
    scene,
    collision,
    effects,
    lights,
    model: flyingModel,
    water,
    pools: world.mission?.pools,
    explode: (explosion, position, owner) => ordnance.explosion(explosion, position, owner),
  });
  // The map's placed power-ups lie where the world put them from the start
  // (src/placed-items.js); the world alone makes them, a page shows records.
  if (headless) spawnPlacedItems(items, propsDef?.items);
  ordnance.water = water;
  // The player's bodies (src/player-body.js): the class played first, from
  // what was fetched above, and any other the first time he takes it. The
  // bindings below are the body he plays now; adopt() swaps them between
  // lives, and wireBody() connects them to the rest of the game.
  const bodies = new PlayerBodies({
    sides, get factory() { return factory; }, json, bin, headless, graphics, scene, collision, animMeta, animBuf,
    maxFlyHeight: world.mission?.maxFlyHeight ?? 200, ordnance, lights, weaponModels, glowModel, anyTexture, hud, effects,
    settings, water, mapName: MAP, camera, simulationCamera, fpAnims, fpAnimBuf,
    jetEffect: (name, sideName) => (sideName === SIDE ? sideFx : otherSides[sideName]?.fx)?.effects?.[name] || effectsDef.effects[name] || [],
    ports: { FirstPersonView, DROIDEKA_OFFSET, JetpackEffects, JetpackNozzles, DroidekaTrail },
  });
  bodies.seed(UNIT, {
    body: { def: actorDef, buf: actorBuf, indexBuf: actorIndexBuf },
    walker: walker ? { meta: walkerMeta, buf: walkerBuf } : null,
    rig: fpRig ? { rig: fpRig, buf: fpBuf, indexBuf: fpIndexBuf } : null,
  });
  let body = bodies.get(UNIT);
  let { actor, clips, soldier, animator, presentation, healthType, bubble, droidekaTrail, jetpackEffects, jetpackNozzles, ribcage, firstPersonView } = body;
  let sideItems = body.sideData.items || {};
  actor.root.visible = true;
  combat = body.combat;
  // The player's body is a unit like any other; main.js steps it.
  const playerUnit = units.add(new Unit({ side: SIDE, className: UNIT, def: soldierDef, actor, soldier, animator, combat, presentation, walker, human: true }));
  playerUnit.team = playerTeam;
  playerUnit.removed = false;
  // Rounds meet soldiers: damage by the soldier's health type, three times
  // on the head (the class HitLocation default), credited to whoever fired;
  // the player's HUD shows where it came from. A body lying dead stops a
  // round and takes nothing.
  const hitFrom = new THREE.Vector3();
  ordnance.units = units;
  ordnance.canHit = null;
  ordnance.onHitUnit = (unit, def, point, owner, part, direction) => {
    if (unit.combat.dead) return;
    const scale = teamDamage(owner, unit) * (part === "head" ? 3 : 1);
    if (part === "head") presentEffect("headshot", point);
    if (scale <= 0) return;
    // A head hit counts for whoever had hurt him last before it (the
    // original's damage record, read before this hit lands).
    if (part === "head") battle.headshot((unit.combat.lastAttacker === "player" ? playerUnit : unit.combat.lastAttacker)?.character ?? null);
    const amount = (def.Damage || 0) * scale;
    // The AI hears a round's hit as coming from 30 units back along it.
    const from = direction ? point.clone().addScaledVector(direction, -30) : null;
    const info = { amount, typed: def[unit.combat.scale] ?? 1, generic: def.ShieldScale ?? 1, known: Boolean(from), point: from };
    unit.combat.damage(amount * info.typed, amount * info.generic, point, owner, info);
    if (unit.human && amount > 0) {
      hitFrom.subVectors(point, soldier.position);
      const yaw = soldier.aimYaw;
      hud.damageFrom(Math.atan2(-hitFrom.x * Math.cos(yaw) + hitFrom.z * Math.sin(yaw), hitFrom.x * Math.sin(yaw) + hitFrom.z * Math.cos(yaw)));
    }
  };
  // A grenade's first touch of anything, for the bots' danger warnings.
  ordnance.onGrenadeContact = (g) => {
    g.contactTime = simTime;
    simEvents.emit("grenade", { position: g.position, team: teamOf(g.owner), source: refOf(g.owner), def: g.def.class || null });
  };
  // What a unit does, for the journal.
  const aimFrom = new THREE.Vector3();
  function instrument(unit) {
    unit.combat.random = gameplayRandom;
    unit.combat.onPresentation = event => simEvents.emit('presentation',{event:{...event,ownerId:unit.id,ownerGeneration:unit.generation}});
    if (!unit.human) unit.onMoved = (h) => ai?.aim?.(unit, h);
    unit.combat.onFired = (shot) => {
      hud.noteShot(unit, shot.weapon);
      unit.origin(aimFrom);
      simEvents.emit("shot", { source: units.ref(unit), team: unit.team, position: aimFrom, direction: unit.combat.shotDir, weapon: shot.weapon.name, repair:!!shot.repair, melee:!!shot.meleeStart, swing:shot.swing||0 });
    };
    unit.combat.onDamaged = (amount, attacker, info) => {
      if (amount > 0 && (attacker === "player" || resolveOrdnanceOwner(attacker) === playerUnit)) hud.hit(units.relation(playerTeam, unit.team));
      simEvents.emit("damage", {
        target: units.ref(unit), team: unit.team, position: unit.position,
        amount: info?.amount ?? amount, typed: info?.typed ?? 1, generic: info?.generic ?? 1,
        known: Boolean(info?.known), point: info?.point ?? null, source: info?.source ?? null, health: amount,
        attacker: refOf(attacker === "player" ? playerUnit : attacker), attackerTeam: teamOf(attacker),
      });
    };
  }
  // Recon droids the scouts throw and fly. They know every side's fire
  // points: a droid flies on after its thrower has taken another class.
  const remotes = new RemoteDroids({
    scene,
    camera,
    collision,
    water,
    lights,
    model: flyingModel,
    firePoints: Object.assign({}, ...Object.values(sides).map((s) => s.firePoints), weaponsDef.firePoints || {}),
    explode: (explosion, position, owner) => ordnance.explosion(explosion, position, owner),
    fire: (def, origin, dir, owner) => ordnance.fire(def, origin, dir, owner),
    // Named by the droid's own class and the gun's channel: the arsenal's
    // weapon classes are copies, absent from the matched combat registry.
    onFired: (shot, position, droid) => simEvents.emit('remoteShot', { droid: droid.def, channel: shot.weapon.channel, position }),
  });
  // The arc caster's lightning (drawn by pages only: a headless world sends
  // its bolts as presentation events), and what it can link: soldiers on
  // foot or shown on an exposed seat, mines and droids. A soldier's bounds
  // (the sphere about his hit shapes) meet the shot's gathering box. The
  // Enhanced look (arc-lightning.js) lights its surroundings and shocks the
  // soldiers it strikes, as this page draws them.
  const lightning = headless ? null : new Lightning(scene, anyTexture, { settings, lights, units, collision, effects, viewScene: graphics.viewScene, bubbleOf: (unit) => (unit === playerUnit ? bubble : unit.bubble) });
  const arcTargets = (owner) => [
    ...[...units.all()].filter((unit) => unit.soldier && unit.combat && !unit.dead && !unit.removed && unit !== owner && (!unit.seat || unit.seat.exposed) && teamDamage(owner, unit) > 0)
      .map((unit) => ({ origin: unit.position, center: unit.sphere.center, radius: unit.sphere.radius, hurt: (amount, from = unit.position) => {
        // From where its arc started: a shield ripples on that side, and the
        // player's HUD shows where it came from (online, from the damage
        // event's source).
        const scaled = amount * teamDamage(owner, unit);
        if (scaled <= 0) return;
        unit.combat.damage(scaled, scaled, from, owner, { amount: scaled, typed: 1, generic: 1, known: true, point: owner?.position ?? null });
        if (unit.human) {
          hitFrom.subVectors(from, soldier.position);
          const yaw = soldier.aimYaw;
          hud.damageFrom(Math.atan2(-hitFrom.x * Math.cos(yaw) + hitFrom.z * Math.sin(yaw), hitFrom.x * Math.sin(yaw) + hitFrom.z * Math.cos(yaw)));
        }
      } })),
    ...items.items.filter((item) => item.mine && !item.gone).map((item) => ({ origin: item.position, hurt: (amount) => items.hurt(item, amount) })),
    ...remotes.droids.filter((droid) => !droid.dead).map((droid) => ({ origin: droid.position, hurt: (amount) => remotes.hurt(droid, amount) })),
    ...[...(networkRemotes?.droids() ?? [])].filter(({droid})=>!droid.dead).map(({context,droid})=>({origin:droid.position,hurt:amount=>context.hurt(droid,amount)})),
  ];

  // Spawning. Reinforcements deploy at the team's own command posts (any post
  // with a spawn path when the map has none for the team), at a random point of
  // the post's authored spawn path. Soldier.spawn settles from just above
  // that height; a separate ceiling search can mistake a bridge deck for a
  // ceiling and move the spawn into its support structure. ?spawn=CP3 picks a post.
  const spawns = world.spawns || {};
  const wanted = (params.get("spawn") || "").toLowerCase();
  let spawnIndex = 0;
  let visual = presentation.position;
  // The view's shake (src/camera-shake.js): the page's, from each explosion
  // it shows, measured to what the camera follows: the droid he flies, the
  // hull he drives, else his body as drawn (at a turret too). The death
  // camera follows nothing and is not shaken.
  const cameraShake = headless ? null : new CameraShake();
  const shakeTarget = { position: new THREE.Vector3(), radius: 0 }, shakeSphere = new THREE.Vector3();
  function cameraTarget() {
    if (deathView.active || !combat || combat.dead || playerUnit.removed) return null;
    const droid = remotes.piloted, hull = driver?.active ? driver.vehicle : null;
    if (droid) {
      shakeTarget.position.copy(droid.position);
      shakeTarget.radius = DROID_RADIUS;
    } else if (hull) {
      shakeTarget.position.copy(hull.root.position);
      shakeTarget.radius = hull.blastSphere(shakeSphere);
    } else {
      shakeTarget.position.copy(visual);
      shakeTarget.radius = (BLAST_BODY[soldier.air ? "stand" : soldier.posture] || BLAST_BODY.stand)[1];
    }
    return shakeTarget;
  }
  function shakeExplosion(explosion, position) {
    return cameraShake ? cameraShake.explode(explosion, position, cameraTarget()) : 0;
  }
  function spawnPosts() {
    const own = posts.owned(playerTeam).filter((post) => spawns[post.desc.spawnPath]);
    return own.length ? own : posts.posts.filter((post) => post.active !== false && spawns[post.desc.spawnPath]);
  }
  function respawn(index = spawnIndex, random = true) {
    driver?.release();
    if (gunner?.active) { gunner.exit(soldier); combat.mounted = false; }
    const choices = spawnPosts();
    let points = Object.values(spawns)[0] || [[0, 10, 0, 0]];
    let label = "";
    if (choices.length) {
      spawnIndex = ((index % choices.length) + choices.length) % choices.length;
      const post = choices[spawnIndex];
      points = spawns[post.desc.spawnPath];
      label = post.label;
    }
    const [x, y, z, yaw] = points[random ? Math.floor(gameplayRandom() * points.length) : 0];
    placePlayer(x, y, z, yaw);
    return label;
  }
  // The player's soldier into the world at a point, alive.
  function placePlayer(x, y, z, yaw) {
    soldier.spawn(x, y, z, yaw);
    playerUnit.generation += 1;
    playerUnit.alive = true;
    playerUnit.dead = false;
    playerUnit.removed = false;
    playerUnit.targetable = true;
    playerUnit.tickCount += 1;
    presentation.reset();
    animator.reset(yaw);
    view.reset();
    simulationView.reset();
    deathView.stop();
  }
  // With the spawn screen he starts with no unit, as the original's
  // Character does: no body in the world, not a target, not counted. His
  // body waits out of sight at his team's first post, which the cameras look
  // at until he spawns (the screen shows its own camera shots meanwhile).
  function parkPlayer() {
    const post = spawnPosts()[0];
    const [x, y, z, yaw] = (post && spawns[post.desc.spawnPath]?.[0]) || Object.values(spawns)[0]?.[0] || [0, 10, 0, 0];
    soldier.spawn(x, y, z, yaw);
    playerUnit.alive = false;
    playerUnit.dead = false;
    playerUnit.removed = true;
    playerUnit.targetable = false;
    combat.dead = true;
    combat.health = 0;
    combat.deathTime = RESPAWN_DELAY;
    combat.updateHeld();
    actor.root.visible = false;
    presentation.reset();
    animator.reset(yaw);
    view.reset();
    simulationView.reset();
    deathView.stop();
  }
  // His body leaves the world CORPSE_LINGER after his death, as a bot's does.
  function removeCorpse() {
    playerUnit.alive = false;
    playerUnit.removed = true;
    playerUnit.targetable = false;
    actor.root.visible = false;
    if (jetFlame) { jetFlame.stop(); jetFlame = null; }
  }
  // The player's way into the battle (src/human-spawn.js): the spawn
  // screen's phases and queued choices; when the URL names his unit, that
  // choice is confirmed at load and after every death instead.
  const humanSpawn = new HumanSpawn({
    policy,
    post: wanted ? posts.posts.find((post) => post.label.toLowerCase() === wanted || post.desc.spawnPath?.toLowerCase().startsWith(wanted))?.name ?? null : null,
    world: {
      team: () => playerTeam,
      dead: () => combat.dead,
      character: () => battle.human,
      get classic() { return ai?.awaitingHuman ? ai : null; },
      roster: (team) => rosterOf(team),
      canSpawn: (className) => bodies.ready(className),
      setTeam: (team) => setHumanTeam(team),
      spawn: (className, post) => spawnHuman(className, post),
      ready: (className, post) => readyHuman(className, post),
      kill: () => killHuman(),
      cancel: () => cancelHuman(),
      over: () => battle.over,
    },
  });
  // The player's end-of-match record tallies from his spawn until his spawn
  // screen opens, as the original's does.
  battle.statsActive = (character) => !character.human || humanSpawn.phase === "alive" || humanSpawn.phase === "dying";
  if (policy.auto) {
    const choices = spawnPosts();
    const pick = choices.findIndex((post) => post.label.toLowerCase() === wanted || post.desc.spawnPath?.toLowerCase().startsWith(wanted || "\0"));
    respawn(Math.max(pick, 0), !wanted);
    humanSpawn.spawned(UNIT, spawnPosts()[spawnIndex]?.name ?? null);
  } else parkPlayer();

  // Input follows the 2004 PC defaults (src/input.js). The pause menu
  // (src/pause) frees the mouse and takes his input (paused). Offline it
  // freezes the world too (frozen); online the server's battle plays on
  // behind it, as the original's network games did, and this page goes on
  // drawing it and playing its sound.
  let paused = false, frozen = false;
  const commandHistory = new CommandHistory();
  const worldControls = new CommandControls();
  let lookPreview = Object.create(soldier);
  lookPreview.aimYaw = soldier.aimYaw; lookPreview.aimPitch = soldier.aimPitch;
  let previewYaw = 0, previewPitch = 0;
  let activeCommand = null;
  const applyLook = (dx, dy) => {
    const scale = activeCommand?.mouseScale ?? settings.mouseScale, invert = activeCommand?.invertMouse ?? settings.get("invertMouse");
    if (combat.dead) return;
    if (remotes.active) remotes.look(dx, dy, scale, invert);
    else if (driver?.active) driver.look(dx, dy, scale, invert);
    else if (gunner.active) gunner.look(dx, dy, scale, invert);
    else (soldier.lookRaw || soldier.look).call(soldier, dx / combat.zoom, dy / combat.zoom, scale, invert);
  };
  const controls = headless ? new HeadlessControls() : new Controls(renderer.domElement, {
    bindings: settings.get("keyBindings"),
    onInput: runtimeProfile || inputTrace ? event => {runtimeProfile?.noteInput(event);inputTrace?.noteInput(event);} : null,
  });
  // The CONTROL page's new keys, from the next key pressed.
  settings.onChange((key, value) => { if (key === "keyBindings") controls.setBindings?.(value); });
  let chatReady = false;
  const chat = headless ? null : createChat({
    controls,
    // T and Y open it at the spawn screen too (a network game's dead talk);
    // the controls come back only when no screen holds them.
    canOpen: () => chatReady && !paused && !menu.isOpen && !game?.scoreboard?.isOpen,
    controlsFree: () => chatReady && !paused && !menu.isOpen && !game?.spawnScreen?.isOpen && !game?.scoreboard?.isOpen,
    // Online the profile's name is what the server will echo back.
    player: () => ({ name: (params.has("network") ? settings.get("playerName") : playerUnit.character?.name) || "PLAYER 1", team: playerTeam }),
  });
  controls.flightScheme = () => seatScheme(driver?.vehicle, settings.get('flightBindings'));
  // The world's keys read his seat's map at the moment they are read (E or
  // a seat key earlier in the tick may have changed the seat).
  worldControls.flightScheme = () => seatScheme(driver?.vehicle, activeCommand?.flightSettings?.bindings);
  // The flight keys go already as the world's (Controls.command), so the
  // world reads them with the defaults ({}), the one profile the network
  // has (DEFAULT_BINDING_PROFILES).
  controls.decorateCommand = command => Object.assign(command, {
    mouseScale: settings.mouseScale, invertMouse: settings.get("invertMouse"), firstPerson,
    aspect: camera.aspect,
    flightTuning: !headless && new URLSearchParams(location.search).has('flightDebug') ? Object.fromEntries(Object.entries(FLIGHT_CLASSES).map(([name,c])=>[name,{...c}])) : undefined,
    flightSettings: { sensitivity: settings.get('flightSensitivity'), invert: settings.get('flightInvertY'), curve: settings.get('flightResponseCurve'), bindings: {} },
    gamepad: !headless && driver?.vehicle?.flying ? (() => { const p = navigator.getGamepads?.()[0]; return p ? { axes: [...p.axes], buttons: p.buttons.map(b => ({pressed:b.pressed})) } : null; })() : null,
  });
  controls.onClear = () => { worldControls.clear(); previewYaw = previewPitch = 0; };
  // The pause menu (src/pause): Esc, or the browser letting go of the mouse,
  // hides the HUD and, offline, freezes the world and fades the game's
  // sound; online the battle and its sound play on behind it. RESUME
  // plays on once the mouse is captured again: from inside the click or key
  // that chose it, or, when the browser refuses (Chrome does, right after
  // Esc let the mouse go), from the next click. ?menu=html swaps in a plain
  // HTML form of the same options (src/menu.js).
  let awaitingPointer = false;
  const pauseHooks = {
    onOpen: () => {
      paused = true;
      frozen = !params.has("network");
      if (frozen && client?.ready) clientFrame(0);
      awaitingPointer = false;
      worldClock.rebase();
      controls.enabled = false;
      controls.clear();
      audio.setPaused(frozen);
    },
    onClose: (resume) => {
      controls.enabled = true;
      controls.clear();
      if (!resume || controls.locked) {
        paused = frozen = false;
        audio.setPaused(false);
        return;
      }
      awaitingPointer = true;
      menu.awaitPointer?.(true);
      controls.lock();
    },
  };
  // Online the battle is the server's: a client cannot restart it, and the
  // machine hosting it asks its own server to (electron/host.js).
  const hostedHere = params.has("network") && ["127.0.0.1", "::1", "localhost"].includes(params.get("address") || "");
  const pauseRestart = !params.has("network") ? undefined : hostedHere && globalThis.bfeDesktop?.host
    ? { label: common.strings?.["ifs.mp.restart"] || "RESTART MATCH", run: () => globalThis.bfeDesktop.host.request("restart").catch((error) => hud.message?.(`Restart failed: ${error.message}`)) }
    : false;
  const menu = headless ? { setPointerLocked() {}, enableRespawn() {} } : params.get("menu") === "html"
    ? new HtmlOptionsMenu(settings, pauseHooks)
    : createPauseMenu({
      hud, settings, controls, strings: common.strings, ...pauseHooks, restart: pauseRestart,
      // The front end's look (src/menus/themes/page) over the battle, unless
      // the URL asks for the original's menus.
      theme: params.get("menus") === "classic" ? undefined : new PageTheme({ overlay: true, settings }),
      viewpoint: { get: () => firstPerson, set: (first) => setPerspective(first) },
      audioPanel: () => window.dispatchEvent(new CustomEvent("audio-panel")),
    });
  controls.onLockChange((locked, requested) => {
    if (locked && awaitingPointer) {
      awaitingPointer = false;
      paused = frozen = false;
      worldClock.rebase();
      audio.setPaused(false);
      menu.awaitPointer?.(false);
    } else if (!locked && !requested && !menu.isOpen) {
      // The browser let the mouse go (Esc, or the page lost focus), not a
      // screen that wanted it (controls.release()).
      menu.open("pointer");
    }
  });

  // The first-person view: the body's arms rig with the first-person
  // weapons (src/player-body.js). The view's motions cue whichever arms he
  // has now.
  view.motion.onCue = (seconds) => firstPersonView?.cue(seconds);

  // Droids and turrets. The droids serve whoever stands near them; E gets the
  // soldier into the turret in front of him, and out again. Turrets start as
  // build pads; destroyed ones blow up, killing whoever mans them.
  live = propsDef
    ? new LiveObjects(scene, propsDef, modelPack(Object.fromEntries(Object.entries(propsDef.models).map(([k, m]) => [k, m.parts])), propsBuf, propsIndexBuf), {
        texture: mapTexture,
        caster: (object) => graphics.caster(object, { far: 70 }),
        collision,
        triangles: propsCollBuf
          ? { positions: new Float32Array(propsCollBuf), indices: new Uint32Array(propsCollIndexBuf), polygons: new Uint32Array(propsCollPolyBuf) }
          : null,
        explode: (explosion, position) => ordnance.explosion(explosion, position, null),
        animationBuffer: propsAnimBuf,
        Trap,
        padOnGround,
        // Who might set an animated prop off: every living soldier's feet,
        // this host's own included (each host steps its own props), and
        // every unit, for the creatures that seize (src/props.js).
        soldiers: () => {
          const out = [];
          if (!combat.dead) out.push(soldier.position);
          for (const unit of factory?.built || []) if (unit.alive && unit.soldier) out.push(unit.soldier.position);
          return out;
        },
        bodies: () => units.all(),
        // Whoever destroyed it is credited, as for a bot at a turret (the
        // original kills the occupant with the turret's own damage record).
        // Killed at the gun, he falls from it (src/boarding.js leaveKilled).
        onDestroyed: (building) => {
          if (gunner?.active && gunner.turret === building) combat.damage(Infinity, Infinity, null, building.lastAttacker ?? null);
        },
        // A droid's pulse that gave someone on foot something: the soldier's
        // gain effects at his feet, as a power-up's (units.js). Recorded in
        // a worker and played on its page like any effect.
        onServe: (target, gave) => {
          const def = target.def || factory?.built.find((unit) => unit.supply === target)?.classDef || {};
          if (gave & 1) presentEffect(def.HealthGainEffect || "Powerup_blue", target.position);
          if (gave & 2) presentEffect(def.AmmoGainEffect || "Powerup_red", target.position);
        },
      })
    : null;
  if (vehicleDef) {
    vehicles = new Vehicles({ scene, data: vehicleDef, flightEnvironment: world.flight || { ceiling: world.mission?.maxFlyHeight ?? 200 }, buffer: vehicleBuf, indexBuffer: vehicleIndexBuf,
      collision, water, audio, maxFlyHeight: world.mission?.maxFlyHeight ?? 200, animationBuffer: vehicleAnimationBuf || new ArrayBuffer(0), texture: vehicleTexture, caster: object => graphics.caster(object, { far: 110 }), uncaster: object => graphics.casters.delete(object), camera: simulationCamera, ordnance, effects, posts, inDeathRegion,
      // He dies with the hull, credited to whoever destroyed it, as the bots
      // aboard are (the original kills the occupant with the hull's damage
      // record).
      onDestroyed: v => {
        // Whoever hurt it last destroyed it, whatever his team (TANK BUSTER).
        battle.vehicleDestroyed((v.lastAttacker === "player" ? playerUnit : v.lastAttacker)?.character ?? null);
        if (driver?.vehicle === v) { driver.release(); combat.damage(Infinity, Infinity, null, v.lastAttacker ?? null); }
      },
    });
    driver = new VehicleDriver({ vehicles, soldier, combat, actor, animator, clips, camera, collision, hud, presentation, hiddenLayer: graphics.hiddenLayer });
    vehicles.attachUnits(units);
    vehicles.flightAssists = settings.get('aiDifficulty') !== 'hard';
    vehicles.humanTarget = (vehicle) => driver.vehicle === vehicle ? humanReticleTarget : null;
    vehicles.random = gameplayRandom;
    driver.onRelease = () => { controls.clear(); worldControls.clear(); };
    driver.aimView = new VehicleCamera(simulationCamera, collision, true);
    hud.vehicles = vehicles;
    if (live) { live.vehicleTargets = vehicles.vehicles; live.playerTeam = playerTeam; }
    hud.preload(Object.values(vehicleDef.classes).flatMap(v => [v.settings.HealthTexture, v.settings.MapTexture,
      ...v.seats.flatMap(s => s.weapons.flatMap(w => [w.def.IconTexture, w.def.ReticuleTexture]))]).filter(Boolean));
    for (const v of Object.values(vehicleDef.classes)) for (const seat of v.seats) for (const w of seat.weapons) {
      if (w.def.ordnance?.LaserTexture) anyTexture(w.def.ordnance.LaserTexture);
    }
  }
  // Thrust, damage, wakes, stomps and wrecks: drawn only, every frame,
  // from whatever hulls and turrets this page shows (src/vehicle-effects.js).
  if (!headless) vehicleEffects = new VehicleEffects({ effects, water, debris });
  destruction?.watch(live);
  // Endor's log traps (src/traps.js): whose they are and whose walkers set
  // them off, from the battle's teams; their crush effect, as any effect.
  if (live?.traps.length) {
    const sideOf = sideOfTeams(world.mission?.teams);
    const teamOfSide = (side) => Number(Object.keys(world.mission?.teams || {}).find((team) => sideOf(team) === side)) || 0;
    live.attachTraps({ sideOf, teamOf: teamOfSide, effect: (name, position) => presentEffect(name, position) });
  }
  hud.preload(
    (propsDef?.instances || []).flatMap((i) => [i.weapon?.IconTexture, i.weapon?.ReticuleTexture, i.settings?.HealthTexture]).filter(Boolean),
  );
  hud.preload(["hud_active_repair_icon", ...(world.commandPosts || []).map((post) => post.mapTexture).filter(Boolean)]);
  gunner = new Gunner({ camera, collision, ordnance, combat, animator });
  gunner.aimCamera = simulationCamera;
  gunner.random = gameplayRandom;
  turretSeats = new TurretSeats({ live, ordnance, collision, units });
  // The destructible posts (src/objectives.js): each its post's building,
  // and a target in the units registry while it stands.
  posts.attachBuildings(live);
  registerObjectives(live, units);
  function mountedShot(object, seat, shot, owner, position, direction) {
    const unit = owner === "player" ? playerUnit : owner;
    simEvents.emit("shot", { source: refOf(unit), mount: refOf(object.aiTarget), seat, team: teamOf(unit), position, direction, weapon: shot.weapon.name, repair:!!shot.repair, melee:!!shot.meleeStart, swing:shot.swing||0 });
    hud.noteShot?.(object.aiTarget ?? unit, shot.weapon.def);
  }
  if (vehicles) {
    vehicles.onFired = mountedShot;
    vehicles.onDamaged = (vehicle, amount, attacker) => simEvents.emit("damage", {
      target: refOf(vehicle.aiTarget), team: vehicle.team, position: vehicle.position,
      amount, typed: 1, generic: 1, known: false, point: null, health: amount,
      attacker: refOf(attacker === "player" ? playerUnit : attacker), attackerTeam: teamOf(attacker),
    });
  }
  turretSeats.onFired = mountedShot;
  // Damage to a turret alerts its gunner (src/props.js Turret.damage): a
  // round's, as a soldier's, with the round's damage and factors and from
  // 30 units back along it (the 2004 game's directional damage: the
  // turret's position less 30 along the round's flat heading); a
  // blast's with no known point (the generic wrapper).
  const turretFrom = new THREE.Vector3();
  for (const turret of live?.turrets || []) turret.onDamaged = (object, amount, attacker, hit = null) => {
    const d = hit?.direction;
    const from = d ? turretFrom.set(d.x, 0.0001, d.z).normalize().multiplyScalar(-30).add(object.position).clone() : null;
    simEvents.emit("damage", {
      target: refOf(object.aiTarget), team: object.aiTarget?.team ?? object.team ?? 0, position: object.position,
      amount: hit?.amount ?? amount, typed: hit?.typed ?? 1, generic: hit?.generic ?? 1, known: Boolean(from), point: from, health: amount,
      attacker: refOf(attacker === "player" ? playerUnit : attacker), attackerTeam: teamOf(attacker),
    });
  };
  for (const system of [vehicles, turretSeats]) if (system) {
    system.onBoard = (unit, object, seat) => simEvents.emit("board", {
      source: refOf(unit), team: unit.team, position: object.position, target: refOf(object.aiTarget), seat,
    });
    system.onUnboard = (unit, object, seat) => simEvents.emit("unboard", {
      source: refOf(unit), team: unit.team, position: unit.position, target: refOf(object.aiTarget), seat,
    });
  }

  // The bots: both sides' classes (sides, above), bodies built ahead, and a
  // brain.
  const unitCenter = new THREE.Vector3();
  factory = new UnitFactory(sides, {
    graphics, scene, collision, animMeta, animBuf, json, bin, ordnance, lights, weaponModels, headless, arcTargets, lightning,
    flashModel: glowModel("muzzleflash"), anyTexture, effects, settings, items, live, units, destruction,
    maxFlyHeight: world.mission?.maxFlyHeight ?? 200,
    onBuilt: instrument,
    deathBlast: (unit) => hooks.presented("deathBlast", unit),
    shieldHit: (unit, amount, source, kind) => hooks.presented("shieldHit", unit, amount, source, kind),
    onDeath: (unit) => {
      // Killed on a seat, he leaves it at once and falls from where he rode.
      leaveKilled(unit);
      // A dead jet trooper no longer jets: the body falls.
      if (unit.soldier.air === "jet") unit.soldier.setAir("fall");
      // The Character lets go of the body at once: it can spawn again while
      // the corpse lingers.
      const character = unit.character;
      if (character) {
        character.diedAt = simTime;
        if (character.unit === unit) character.unit = null;
      }
      characterDied(character, unit.team);
      // The last soldier to hurt him is credited, then the death counts
      // (Battle.credit).
      const attacker = unit.combat.lastAttacker === "player" ? playerUnit : unit.combat.lastAttacker;
      const killer = attacker?.character ?? null;
      battle.credit(character, killer, killer ? units.relation(killer.team, unit.team) : 0);
      simEvents.emit("death", { target: units.ref(unit), team: unit.team, position: unit.position, attacker: refOf(attacker), attackerTeam: teamOf(attacker) });
      hud.killMessage({ killer: attacker, victim: unit, relationToViewer: radarRelation(playerTeam, teamOf(attacker), (a, b) => units.relation(a, b), (team) => units.locals(team)) });
      // The dead may drop a power-up, from the middle of the body.
      unitCenter.copy(unit.position);
      unitCenter.y += BODY_MIDDLE;
      items.dropFor(unit.classDef.drops, sides[unit.side].items, unitCenter, unit.velocity, unit.soldier.aimYaw);
      // Enhanced: a droid left standing shorts out, then comes apart on screen.
      hooks.presented("died", unit);
    },
    presentEffect,
    onBlowApart: (unit, health) => {
      unit.actor.root.updateMatrixWorld(true);
      unitCenter.copy(unit.position);
      unitCenter.y += unit.walker ? 0 : 0.9;
      if (unit.classDef.explosion) ordnance.explosion(unit.classDef.explosion, unitCenter.clone(), unit, destruction?.explosion(unit.classDef.explosion) ?? unit.classDef.explosion);
      hooks.presented("blowApart", unit, health);
      unit.actor.root.visible = false;
    },
  });
  destruction?.prepare(Object.values(sides).flatMap((s) => Object.values(s.units)));
  // With the spawn screen, every class the player can take on either team,
  // whatever the bots do: its body, a droideka's clips, its first-person
  // arms, its HUD icons, and the textures its body, arms and weapons show.
  // One that fails to load isn't offered. When the URL names his unit it
  // never changes, so only that one is loaded (and the bots' own, when the
  // AI runs).
  {
    const rosterClasses = [...new Set([1, 2].flatMap((team) => playableClasses(teams[team])))].filter((className) => factory.sideOf(className));
    await Promise.all(rosterClasses.map((className) => bodies.prepare(className).catch((error) => {
      console.warn(`${className} can't be played: ${error.message}`);
    })));
    for (const className of rosterClasses) {
      const sideName = factory.sideOf(className);
      preloadClass(sides[sideName].units[className], sides[sideName].items);
      bodies.preloadTextures(className);
    }
  }
  // Each team's Characters (the player is one of his team's).
  battle.populate(playerTeam);
  if (battle.human) {
    battle.human.unit = policy.auto ? playerUnit : null;
    battle.human.className = UNIT;
    playerUnit.character = battle.human;
  }
  const random = new ClassicRandom(SEED);
  const classesFor = (team) => (teams[team]?.classes || []).filter(([className]) => factory.sideOf(className));
  // A death costs the team a reinforcement: through the Classic spawn
  // manager when it runs the battle (it keeps the living counts too), else
  // directly.
  function characterDied(character, team) {
    if (ai?.died) ai.died(character);
    else battle.spend(team);
  }
  // A Character into the world as `className` at `point` facing `yaw`. The
  // human's Character is the player's own soldier, placed there.
  function spawnUnit(character, className, post, point, yaw) {
    if (character.human) {
      // His body for the class, if he played another last time.
      const next = bodies.get(className);
      if (next && next !== body) adopt(next);
      driver?.release();
      if (gunner?.active) { gunner.exit(soldier); combat.mounted = false; }
      placePlayer(point[0], point[1], point[2], yaw);
      revivePlayer();
      character.unit = playerUnit;
      character.className = playerUnit.className;
      playerUnit.character = character;
      simEvents.emit("spawn", { target: units.ref(playerUnit), team: playerUnit.team, position: playerUnit.position, className: playerUnit.className, post: post?.name ?? null });
      humanSpawn.spawned(playerUnit.className, post?.name ?? null);
      return playerUnit;
    }
    if (!factory.prepared(className)) return null;
    const unit = factory.acquire(className);
    unit.team = character.team;
    unit.combat.team = character.team;
    unit.character = character;
    character.unit = unit;
    character.className = className;
    unit.spawn(point[0], point[1], point[2], yaw);
    unit.hero = Boolean(character.hero);
    unit.waterContact = {};
    simEvents.emit("spawn", { target: units.ref(unit), team: unit.team, position: unit.position, className, post: post?.name ?? null });
    ai?.spawned?.(character);
    return unit;
  }
  // Sight: terrain, buildings, props, vehicles, and soldiers other than the
  // two named, between two points.
  const losDir = new THREE.Vector3();
  const losHit = makeHit();
  const groundHit = { height: 0, normal: new THREE.Vector3() };
  const losUnitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
  const losExclude = [null, null];
  function los(from, to, a = null, b = null) {
    losDir.subVectors(to, from);
    const span = losDir.length();
    if (span < 1e-6) return true;
    losDir.divideScalar(span);
    const bodyOf = (unit) => unit?.vehicle?.body ?? unit?.turret?.bodies?.built ?? unit?.building?.bodies?.built ?? unit?.seat?.vehicle?.body ?? unit?.seat?.turret?.bodies?.built ?? null;
    if (collision.cast(from, losDir, span, losHit, bodyOf(a)) && (!bodyOf(b) || losHit.body !== bodyOf(b))) return false;
    losExclude[0] = a;
    losExclude[1] = b;
    return !units.raycast(from, losDir, span, losExclude, losUnitHit);
  }
  if (AI_MODE !== "off") {
    // Every team the mission fields: both armies and the locals (Tatooine's
    // Tusken Raiders and Jawas, Naboo's Gungans, ...). A locals' class that
    // won't load fields nobody; the armies' must.
    const fielded = Object.keys(teams).map(Number);
    const armies = new Set();
    for (const team of [playerTeam, enemyTeam]) for (const [className] of classesFor(team)) armies.add(className);
    if (HEROES) for (const team of Object.values(teams)) if (team.hero && factory.sideOf(team.hero)) armies.add(team.hero);
    const locals = new Set(fielded.flatMap((team) => classesFor(team).map(([className]) => className)).filter((className) => !armies.has(className)));
    await Promise.all([...armies].map((className) => factory.prepare(className)));
    await Promise.all([...locals].map((className) => factory.prepare(className).catch((error) => console.warn(`${className} won't be fielded: ${error.message}`))));
    // Bodies for each team's roster, built now so a spawn never stalls.
    for (const team of fielded) {
      // Half again as many as the roster, so the dead can respawn while
      // their bodies are still on the field.
      for (const [className, count] of classesFor(team)) {
        if (factory.prepared(className)) for (let i = 0; i < Math.ceil(count * 1.5); i++) factory.release(factory.build(className));
      }
    }
    // What the AI sees of the battle (docs/ai.md, "World services for AI").
    const aiWorld = {
      get time() {
        return simTime;
      },
      random,
      collision,
      los,
      units,
      relation: (a, b) => units.relation(a, b),
      posts,
      battle,
      mission: world.mission || {},
      spawns,
      camera: simulationCamera,
      events: simEvents,
      spawnUnit,
      factory,
      classesFor,
      vehicleSystem: vehicles,
      turretSeats,
      live,
      items,
      serviceCandidates: createServiceCandidates(units, live, vehicles),
      mineGroundHeight: (point) => collision.ground(point.x, point.z, point.y + 100, groundHit)?.height ?? point.y,
    };
    ai = await createAI(AI_MODE, aiWorld, {
      map: MAP,
      sides: Object.keys(sides),
      playerTeam,
      enemyTeam,
      tier: TIER,
      heroes: HEROES,
      unitScale: UNIT_SCALE,
      seed: SEED,
      stub: { battle, units, posts, spawns, spawnUnit, los, random: () => random.uniform(), classesFor, time: () => simTime },
    }).catch((error) => {
      console.error(`AI (${AI_MODE}) failed to start: ${error.message}`);
      console.error(error);
      return null;
    });
    randomB = aiWorld.randomB;
    if (ai?.humanReady && battle.human) {
      playerUnit.character = battle.human;
      battle.human.className = UNIT;
    }
  }
  // Which posts the player may spawn at needs each post's spawn paths (and
  // its path for allies). The Classic AI attaches them from the map's
  // ai-mission.json; otherwise they're attached here, from that file when
  // the map has one, else from the map's own spawn paths.
  if (!ai?.spawning) {
    const mission = selectAIMission(await fetch(`${mapDir}/ai-mission.json`).then((r) => (r.ok ? r.json() : null)).catch(() => null), world.mission);
    attachPostAI(posts.posts, mission, spawns);
    // The command vehicles' posts (src/mobile-posts.js), with their classes'
    // spawn points, weights and ValueBleed from the same file.
    posts.attachVehicles(vehicles, mission?.commandClasses);
    // The roster's bots are named as the Classic AI's would be.
    battle.nameBots(mission?.names, (team) => factionOf(teams[team]?.name));
    units.setTeams(missionTeams(mission?.scripts?.[mission.mission]?.calls ?? callsFromMapMission(world.mission)));
  } else {
    // Who is whose friend (the mission's SetTeamAs*), each team's faction and
    // whom its soldiers capture for, as the spawn manager read them: for the
    // fighting, the posts, the AI and the HUD alike, here, in a world worker
    // and on a server.
    units.setTeams(teamTable(ai.spawning.teams));
  }
  // Leaving the battlefield (src/battlefield.js): the map's boundary, the
  // Classic AI's or read for it. The world keeps each Character's count; the
  // page that shows the player works out his warning.
  const battlefield = new Battlefield(ai?.nav?.boundary ?? await loadBoundary(`${mapDir}/ai-nav.json`));
  const leavingWarning = headless ? null : new LeavingWarning();
  // The player, as a droid serves him (PowerupStation): marked player so the
  // HUD can tell a droid serving him from one serving a bot beside him.
  const supplied = {
    player: true,
    get def() {
      return soldierDef;
    },
    get position() {
      return soldier.position;
    },
    get health() {
      return combat.health;
    },
    set health(value) {
      combat.health = value;
    },
    get maxHealth() {
      return combat.maxHealth;
    },
    get weapons() {
      return combat.arsenal.weapons;
    },
  };
  const eyePoint = new THREE.Vector3();
  function toggleTurret() {
    if (driver?.active) { driver.exit(); return; }
    if (gunner.active) {
      gunner.exit(soldier);
      animator.reset(soldier.aimYaw);
      combat.mounted = false;
      return;
    }
    if (combat.dead) return;
    // What his E reaches, by the rules every boarder follows (src/boarding.js).
    const mount = mountInReach({ vehicles, live }, playerUnit, true);
    const vehicle = mount?.vehicle;
    if (vehicle && driver.enter(vehicle)) { if (vehicle.flying) setPerspective(Boolean(settings.get('flightFirstPerson'))); return; }
    const turret = mount?.turret;
    if (turret) {
      // Taking over from a bot gunner: his crew sets him down first.
      if (turret.lease && !turret.lease.released && !personLease(turret.lease)) turret.crew?.vacate(turret.lease);
      gunner.enter(turret, soldier);
      combat.mounted = true;
      hud.message(turret.label);
    }
  }

  // The jetpack's flames (the class's JetEffect; its effects come with the
  // body), hung from the ribcage while the soldier hovers; when he stops
  // they die out.
  let jetFlame = null;
  const flameAt = new THREE.Vector3();
  const flameTurn = new THREE.Quaternion();
  const flameScale = new THREE.Vector3();
  function updateJetFlame(dt) {
    const name = soldierDef.JetEffect;
    const active = soldier.air === "jet" && name && ribcage && !combat.dead;
    const hot = active || jetpackEffects?.throttle > 0;
    if (hot) {
      ribcage.updateWorldMatrix(true, false);
      ribcage.matrixWorld.decompose(flameAt, flameTurn, flameScale);
    }
    jetpackEffects?.update(dt, {
      active, position: flameAt, quaternion: flameTurn, velocity: soldier.velocity,
      nozzleFrames: hot ? jetpackNozzles?.update() : null,
    });
    if (active && !jetpackEffects?.enhanced) {
      if (!jetFlame) jetFlame = effects.play(name, flameAt, null, soldier.velocity, flameTurn);
      else jetFlame.move(flameAt, flameTurn, soldier.velocity);
      jetFlame?.attachNozzles?.(jetpackNozzles?.update());
    } else if (jetFlame) {
      jetFlame.stop();
      jetFlame = null;
    }
  }

  // P switches between the chase camera and the soldier's own eyes. In first
  // person his body still casts its shadow but isn't drawn; dead, it is
  // drawn in either view, as the original goes back to the chase camera at
  // a death and the death camera looks at the body.
  let firstPerson = false;
  let bodyLaid = null; // [actor, hidden] as last laid out
  function layBody(force = false) {
    const hidden = combat.usesEyeView && !combat.dead && !remotes.active;
    if (!force && bodyLaid?.[0] === actor && bodyLaid[1] === hidden) return;
    bodyLaid = [actor, hidden];
    const layer = hidden ? graphics.hiddenLayer : 0;
    // The body's shadow proxy (actor.js) draws only in the shadow pass.
    actor.root.traverse((object) => object.layers.set(object === actor.shadow ? graphics.hiddenLayer : layer));
  }
  function setPerspective(first) {
    firstPerson = first;
    combat.firstPerson = first;
    layBody(true);
    // A new view: the temporal upscaler's history no longer lines up.
    graphics.resetHistory?.();
  }
  setPerspective(params.get("view") === "1st");
  audio.attach({ camera, soldier, combat, ordnance, controls, posts, battle, collision, teams, actor, water, live, gunner, items, remotes, units, team: playerTeam, foe: enemyTeam, unit: UNIT, def: soldierDef });

  // Every hull's own sounds read the hulls this page shows (src/audio/vehicles.js).
  audio.retarget({ vehicles });

  // Killed: a reinforcement is spent, and the soldier deploys again after
  // the death clip. Without the spawn screen the URL's choice is confirmed
  // again: the Classic AI's spawn manager spawns the player like everyone
  // else (ready now, placed by spawnUnit on its next update), else he is
  // placed at once. With the screen, its choice comes through humanSpawn.
  function onRespawn() {
    if (!policy.auto) return;
    if (ai?.humanReady) {
      ai.humanReady(battle.human, { className: UNIT, post: spawnPosts()[spawnIndex] });
      return;
    }
    respawn();
    revivePlayer();
    humanSpawn.spawned(playerUnit.className, spawnPosts()[spawnIndex]?.name ?? null);
  }
  // A fresh life's loadout draws its spread from a stream of its own, so
  // the world worker and replays shoot alike (the first life's weapons draw
  // from the gameplay stream, as they always have).
  const playerSpread = () => streams.get("player-weapons").uniform();
  function revivePlayer() {
    combat.reset();
    for (const weapon of combat.arsenal.weapons) weapon.random = playerSpread;
    actor.root.visible = true;
    hud.wake();
  }
  // A droid blowing apart: its explosion, and its pieces flying in place of
  // the body.
  function onBlowApart(health) {
    actor.root.updateMatrixWorld(true);
    const center = soldier.position.clone();
    center.y += walker ? 0 : 0.9;
    if (soldierDef.explosion) ordnance.explosion(soldierDef.explosion, center, "player", destruction?.explosion(soldierDef.explosion) ?? soldierDef.explosion);
    hooks.presented("blowApart", playerUnit, health);
    actor.root.visible = false;
  }
  hud.wake();
  // Whom the death camera turns to: whoever killed him, unless he did it
  // himself (his own rounds, his droid's, his grenade's). Like the
  // original's, it follows the killer's Character from then on: his body,
  // or the vehicle or turret he sits in, while he has one.
  let deathKiller = null;
  const deathKillerEyes = new THREE.Vector3();
  function killerOf(attacker) {
    if (!attacker || attacker === "player" || attacker === playerUnit) return null;
    if (battle.human && attacker.character === battle.human) return null;
    // A hull credited by itself (its wreck's blast): it while it lives (the
    // same hull's life: a wreck comes back as the same object).
    if (!attacker.soldier) return attacker.seatOrigin && attacker !== driver?.vehicle ? { vehicle: attacker, life: attacker.aiTarget?.generation ?? 0 } : null;
    return { character: attacker.character ?? null, unit: attacker, generation: attacker.generation };
  }
  // The killer's eyes now, or null while he has no body.
  function killerEyes() {
    const killer = deathKiller;
    if (!killer) return null;
    if (killer.vehicle) return killer.vehicle.alive && (killer.vehicle.aiTarget?.generation ?? 0) === killer.life ? killer.vehicle.seatOrigin(0, deathKillerEyes) : null;
    const unit = killer.character ? killer.character.unit : killer.unit.generation === killer.generation ? killer.unit : null;
    return unit?.alive && !unit.removed ? unit.origin(deathKillerEyes) : null;
  }
  // The death camera takes over from the world's camera as it stands: his
  // chase view or eyes, or the vehicle's, turret's or droid's he had. Its
  // field of view is that view's, unzoomed (zoomed, the base was
  // FirstPersonFOV), after the chase camera's one step toward a dead
  // soldier's (deadFov). A droideka has none (the original game starts it only
  // from a soldier's death), so his chase camera stays on the wreck.
  function startDeathCamera() {
    deathKiller = killerOf(combat.lastAttacker);
    if (walker) {
      deathView.stop();
      return;
    }
    const across = 2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(simulationCamera.fov) / 2) * simulationCamera.aspect);
    const fov = simulationZoom > 1 ? simulationView.firstFov : across;
    deathView.start(simulationCamera, soldier.position, Boolean(deathKiller), deadFov(fov, worldClock.dt));
    deathView.blame(deathKiller, battle.characters, vehicles?.vehicles);
  }
  // Online the page takes no damage itself: the server's damage and death
  // records (network-combat-feedback.js) name who hurt him, as the world's
  // damage record does; the one for his death names the killer. attacker:
  // the unit, null for none, undefined for one this page doesn't have.
  let attackerTick = -Infinity;
  function noteAttacker(attacker, fatal, tick) {
    if (!fatal && tick < attackerTick) return;
    attackerTick = Math.max(attackerTick, tick);
    if (attacker !== undefined) combat.lastAttacker = attacker;
    if (!fatal || !deathView.active) return;
    deathKiller = killerOf(combat.lastAttacker);
    deathView.killed = Boolean(deathKiller);
    deathView.blame(deathKiller, battle.characters, vehicles?.vehicles);
  }
  function onDeath() {
    startDeathCamera();
    // Killed on a seat, he leaves it where he rode, his death under way.
    if (!leaveKilled(playerUnit)) driver?.release();
    // Dead, he no longer jets (the original's dead state): the body falls.
    if (soldier.air === "jet") soldier.setAir("fall");
    playerUnit.alive = false;
    playerUnit.dead = true;
    const character = battle.human;
    if (character) character.diedAt = simTime;
    const attacker = combat.lastAttacker === "player" ? playerUnit : combat.lastAttacker;
    const killer = attacker?.character ?? null;
    battle.credit(character, killer, killer ? units.relation(attacker.team, playerTeam) : 0);
    hud.killMessage({ killer: attacker, victim: playerUnit, relationToViewer: radarRelation(playerTeam, teamOf(attacker), (a, b) => units.relation(a, b), (team) => units.locals(team)) });
    characterDied(battle.human, playerTeam);
    simEvents.emit("death", { target: units.ref(playerUnit), team: playerTeam, position: soldier.position, attacker: refOf(combat.lastAttacker), attackerTeam: teamOf(combat.lastAttacker) });
    // Control comes back from a droid; it flies on, unpiloted.
    remotes.exit();
    // The dead may drop a power-up, from the middle of the body.
    if (!gunner.active) {
      const center = soldier.position.clone();
      center.y += BODY_MIDDLE;
      items.dropFor(soldierDef.drops, sideItems, center, soldier.velocity, soldier.aimYaw);
    }
    hooks.presented("died", playerUnit);
    humanSpawn.died();
  }
  // The player, to the items: power-ups he can take (and, for mines, what
  // side he's on), in whichever body he has.
  const bodyMiddle = new THREE.Vector3();
  const itemTargets = [
    {
      get position() {
        return soldier.position;
      },
      center: bodyMiddle,
      radius: BODY_RADIUS,
      get velocity() {
        return soldier.velocity;
      },
      relation: (team) => units.relation(playerTeam, team),
      give: (def) => {
        const given = givePowerup(def, {
          get health() {
            return combat.health;
          },
          set health(value) {
            combat.health = value;
          },
          maxHealth: combat.maxHealth,
          addHealth: soldierDef.AddHealth,
          weapons: combat.arsenal.weapons,
          healthType,
        });
        if (given.health) effects.play(soldierDef.HealthGainEffect || "Powerup_blue", soldier.position);
        if (given.ammo) effects.play(soldierDef.AmmoGainEffect || "Powerup_red", soldier.position);
        return given.health || given.ammo;
      },
    },
  ];

  remotes.aimCamera = simulationCamera;
  for (const system of [ordnance, items, remotes]) system.random = gameplayRandom;

  // The body he plays, connected to the rest of the game: the Unit and his
  // team, what his weapons reach (mines and power-ups, droids, lightning,
  // turrets, vehicles), what they report, the first-person arms, and how his
  // death and respawn go. For the first body here, and for each body adopt()
  // gives him after.
  function wireBody() {
    combat.owner = playerUnit;
    combat.team = playerTeam;
    combat.items = items;
    combat.remotes = remotes;
    remotes.owner = playerUnit;
    combat.lightning = lightning;
    combat.arcTargets = arcTargets;
    combat.live = live;
    combat.vehicles = vehicles;
    combat.effects = effects;
    combat.view = firstPersonView;
    combat.onRespawn = onRespawn;
    combat.onBlowApart = onBlowApart;
    combat.onDeath = onDeath;
    instrument(playerUnit);
    combat.random = gameplayRandom;
    for (const weapon of combat.arsenal.weapons) weapon.random = gameplayRandom;
    for (const weapon of soldierDef.weapons || []) {
      const bolt = effects.lightning[weapon.ordnance?.LightningEffect];
      if (bolt) lightning?.material(bolt.Texture);
    }
    if (firstPersonView && firstPersonView.root.parent !== graphics.viewScene) graphics.viewScene.add(firstPersonView.root);
    // A droideka: its shield's hits on its bubble, and its death in blasts.
    if (walker) {
      soldier.onShieldHit = (amount, source, kind) => hooks.presented("shieldHit", playerUnit, amount, source, kind);
      soldier.onDeathBlast = () => hooks.presented("deathBlast", playerUnit);
      soldier.onExploded = () => combat.onBlowApart(0);
    }
  }
  wireBody();

  // He takes another class's body (src/player-body.js), only between lives,
  // while he has no unit. The old body is put away; the new one lies where it
  // lay, dead, until he spawns. Every stored reference to the body moves
  // with it: the cameras, the vehicle and turret seats, the sounds, the
  // first-person scene. His Unit stays the same object.
  function adopt(next) {
    if (!next || next === body || !combat.dead) return false;
    const old = body;
    old.actor.root.visible = false;
    if (old.bubble) old.bubble.mesh.visible = false;
    old.droidekaTrail?.clear();
    old.jetpackEffects?.clear();
    if (jetFlame) { jetFlame.stop(); jetFlame = null; }
    if (old.firstPersonView) graphics.viewScene.remove(old.firstPersonView.root);
    next.soldier.position.copy(soldier.position);
    next.soldier.velocity.set(0, 0, 0);
    next.soldier.aimYaw = soldier.aimYaw;
    next.soldier.aimPitch = soldier.aimPitch;
    next.combat.dead = true;
    next.combat.health = 0;
    next.combat.deathTime = combat.deathTime;
    next.combat.updateHeld();
    body = next;
    ({ def: soldierDef, actor, clips, soldier, animator, walker, presentation, combat, healthType, bubble, droidekaTrail, jetpackEffects, jetpackNozzles, ribcage, firstPersonView } = next);
    sideItems = next.sideData.items || {};
    playerUnit.setBody({ side: next.side, className: next.className, def: soldierDef, actor, soldier, animator, combat, presentation, walker });
    presentation.reset();
    visual = presentation.position;
    lookPreview = Object.create(soldier);
    lookPreview.aimYaw = soldier.aimYaw; lookPreview.aimPitch = soldier.aimPitch;
    previewYaw = previewPitch = 0;
    view.setDef(soldierDef);
    simulationView.setDef(soldierDef);
    if (driver) Object.assign(driver, { soldier, combat, actor, animator, clips, presentation });
    gunner.combat = combat;
    gunner.animator = animator;
    for (const key of Object.keys(waterContact)) delete waterContact[key];
    wireBody();
    setPerspective(firstPerson);
    audio.retarget({ soldier, combat, actor, unit: next.className, def: soldierDef });
    return true;
  }

  // He moves to the other team between lives (the spawn screen's team
  // switch), everywhere his team is kept (src/human-spawn.js
  // switchHumanTeam). No ticket is spent: his death already cost one.
  // `announce` false: the page following the world's switch, whose own
  // teamChanged comes with the tick's calls (it would be heard twice).
  function setHumanTeam(team, announce = true) {
    if (team === playerTeam || !teams[team] || !combat.dead) return false;
    return changeHumanTeam(team, false, announce);
  }
  function validateNetworkHumanTeam(team) {
    if (!client || (team !== 1 && team !== 2) || !teams[team]) throw Error('Unsupported authoritative human team');
  }
  function applyNetworkHumanTeam(team) {
    validateNetworkHumanTeam(team);
    if (team === playerTeam) return false;
    return changeHumanTeam(team, true);
  }
  function changeHumanTeam(team, authoritative, announce = true) {
    const previous = playerTeam;
    const moved = (authoritative ? applyAuthoritativeHumanTeam : switchHumanTeam)({
      setTeams: (own, foe) => { playerTeam = own; enemyTeam = foe; },
      character: battle.human,
      ai: ai?.switchTeam ? ai : null,
      posts,
      battle,
      live,
      combats: bodies.all().map((each) => each.combat),
      unit: playerUnit,
      audio,
    }, team);
    if (moved && announce) humanSpawn.teamChanged(team, previous);
    return moved;
  }

  // The classes the player may take on a team: its roster in mission order,
  // without its hero, those whose bodies loaded. They don't change once the
  // map has loaded, so teams 1 and 2 are kept then (rosters, below): the
  // screen asks every frame.
  let rosters = null;
  function rosterOf(team) {
    return rosters?.get(team) ?? playableClasses(teams[team]).filter((className) => bodies.ready(className));
  }
  // Team relations for spawn posts: the Classic spawn manager's (the
  // mission's SetTeamAs* calls) when it runs, else the registry's.
  function spawnRelation(a, b) {
    return ai?.spawning ? ai.spawning.relation(a, b) : units.relation(a, b);
  }
  // Where a soldier may be sent to spawn: every post, the living command
  // vehicles' as they are now (src/mobile-posts.js), and the landed
  // transports.
  function spawnChoices() {
    return [...posts.all().filter((post) => post.active !== false), ...transportPosts(vehicles)];
  }

  // Without the Classic AI: into the world now as className at the named
  // post, at a random point of its spawn path (src/human-spawn.js). A post
  // that no longer serves gives way to the eligible one nearest it, or the
  // front line; with none eligible, his team's first post with a spawn path,
  // as before the screen. False if there is no post at all.
  function spawnHuman(className, postName) {
    const choices = spawnChoices();
    const named = choices.find((post) => post.name === postName) || null;
    const post = choosePost(choices, playerTeam, named, spawnRelation, gameplayRandom) || spawnPosts()[0];
    const next = bodies.get(className);
    if (!post || !next) return false;
    if (next !== body) adopt(next);
    const fallback = friendlySpawn(post, playerTeam, spawnRelation) ? null : spawns[post.desc.spawnPath];
    const [x, y, z, yaw = 0] = fallback ? fallback[Math.floor(gameplayRandom() * fallback.length)] : spawnPointAt(post, playerTeam, gameplayRandom);
    driver?.release();
    if (gunner?.active) { gunner.exit(soldier); combat.mounted = false; }
    placePlayer(x, y, z, yaw);
    revivePlayer();
    spawnIndex = Math.max(spawnPosts().indexOf(post), 0);
    if (battle.human) {
      battle.human.unit = playerUnit;
      battle.human.className = className;
      playerUnit.character = battle.human;
    }
    simEvents.emit("spawn", { target: units.ref(playerUnit), team: playerUnit.team, position: playerUnit.position, className, post: post.name ?? null });
    humanSpawn.spawned(className, post.name ?? null);
    return true;
  }
  // With the Classic AI: the choice goes to the spawn manager, which places
  // him on its next update. A choice it took while he waited for a post gives
  // way to this one (the screen's BACK, then SPAWN again).
  function readyHuman(className, postName) {
    const character = battle.human;
    if (!character?.spawn || character.unit) return false;
    const transport = transportPosts(vehicles).find(p => p.name === postName && p.team === playerTeam);
    if (transport && character.spawn.timer <= 0) return spawnHuman(className, postName);
    character.spawn.ready = false;
    const post = postName ? posts.all().find((each) => each.name === postName) : undefined;
    ai.humanReady(character, { className, post });
    return character.spawn.ready;
  }
  // The screen's BACK while a choice waits: with the Classic AI the spawn
  // manager lets go of it (the Character's ready flag), as the original's
  // uncommit does; he stays on the screen.
  function cancelHuman() {
    const character = battle.human;
    if (ai?.spawning && character?.spawn && !character.unit) character.spawn.ready = false;
  }
  // The pause menu's RESPAWN: a normal death, out of any vehicle or turret
  // first (a ticket is spent, then the timer and the screen).
  function killHuman() {
    if (combat.dead) return;
    driver?.release();
    if (gunner?.active) { gunner.exit(soldier); combat.mounted = false; }
    combat.damage(Infinity);
  }

  const input = { forward: 0, right: 0, turn: 0, jump: false, crouch: false, prone: false };
  // The keys while flying a droid.
  const droidInput = { forward: 0, right: 0, turn: 0 };
  const mineTargets = [];
  const served = [];
  // This tick's spawn screen choices (command.human), applied in updateBattle.
  let tickHuman = null;
  // How far this tick's mouse look turned him (a page showing a worker's
  // battle keeps it out of the body's interpolation, as previousYaw does).
  let tickLookYaw = 0;
  function step(dt, tick, recordedCommand = null) {
    runtimeProfile?.step(dt);
    const command = recordedCommand || controls.command(tick, commandHistory.epoch, playerUnit.generation);
    activeCommand = command;
    tickHuman = command.human || null;
    // The press this tick's rounds answer, by its id (WorkerHost.edges): the
    // page that showed them at once draws the world's copies only once.
    playerUnit.networkShots = command.fire?.filter(edge=>edge.pressed) ?? [];
    playerUnit.networkShot = null;
    if (typeof command.firstPerson === "boolean" && command.firstPerson !== firstPerson) setPerspective(command.firstPerson);
    if (Number.isFinite(command.aspect) && command.aspect > 0 && simulationCamera.aspect !== command.aspect) {
      simulationCamera.aspect = command.aspect;
      simulationView.applyFov(command.aspect);
    }
    commandHistory.put(command);
    if (!worldControls.accept(command)) throw new Error("stale world command");
    if (command.flightTuning) for (const [name, values] of Object.entries(command.flightTuning)) if (FLIGHT_CLASSES[name]) for (const [key,value] of Object.entries(values)) if (key in FLIGHT_CLASSES[name] && Number.isFinite(value) && value>0) FLIGHT_CLASSES[name][key]=value;
    const fs = command.flightSettings || {};
    if (driver) { driver.flightSensitivity = fs.sensitivity ?? 1; driver.flightInvert = Boolean(fs.invert); }
    const yawBeforeLook = soldier.aimYaw;
    for (const [dx, dy] of command.look) applyLook(dx, dy);
    tickLookYaw = soldier.aimYaw - yawBeforeLook;
    // The view has already previewed these raw counts. Do not interpolate
    // them backwards after the tick acknowledges them; only movement/keyboard
    // turns belong in the presentation's previous-to-current yaw interval.
    presentation.previousYaw += soldier.aimYaw - yawBeforeLook;
    previewYaw = previewPitch = 0;
    const tickControls = worldControls;
    water?.step(dt);
    simTime = tick / 60;
    simEvents.time = simTime;
    input.forward = tickControls.axis("forward");
    input.right = tickControls.axis("right");
    input.turn = tickControls.axis("turn");
    droidInput.forward = input.forward;
    droidInput.right = input.right;
    droidInput.turn = input.turn;
    for (const [action, kind, label] of [["squadMoveOut", 0, "moveout"], ["squadHold", 2, "hold"], ["squadFollow", 3, "follow"], ["squadAtEase", 1, "atease"]]) {
      if (tickControls.take(action) && !combat.dead && ai?.playerCommand) {
        ai.playerCommand(kind); hud.squadCommand(label);
      }
    }
    if (!remotes.active && !walker && tickControls.take("use")) toggleTurret();
    if (!game.serverWorld) stepPlayer(dt);
    playerUnit.tickCount += 1;

    combat.eye(soldier.position, simulationEye);
    // Dead, the view is never his own eyes. The death camera follows the
    // body while it lies in the world, and the killer.
    const eyes = combat.usesEyeView && !combat.dead;
    simulationZoom = 1;
    if (deathView.active) {
      deathView.update(dt, playerUnit.removed ? null : soldier.position, killerEyes());
      deathView.apply(simulationCamera);
    } else if (remotes.active) remotes.updateCamera(dt, firstPerson, true);
    else if (driver?.active) driver.aimView.update(driver.vehicle, dt, firstPerson);
    else if (gunner.active) gunner.updateCamera(dt, 1, firstPerson, true);
    else {
      simulationZoom = combat.zoom;
      simulationView.update(dt, soldier.position, soldier.aimYaw, soldier.aimPitch,
        soldier.air ? "stand" : soldier.posture, combat.zoom, eyes, eyes ? simulationEye : null);
    }
    simulationCamera.updateMatrixWorld();
    vehicles?.restoreTickPose?.();
    ai?.update(dt);
    for (const unit of factory?.built || []) {
      unit.tick(dt);
      if (water && unit.alive && !unit.seat && !unit.held && !unit.walker) {
        unit.waterContact ??= {};
        const hurt = water.soldierStep(dt, unit.waterContact, unit.position, unit.combat.pivot, unit.combat.origin, unit.velocity,
          (at) => presentEffect("watersplash_md", at));
        unit.combat.gunUnderwater = Boolean(unit.waterContact.gunUnder);
        if (hurt) unit.combat.damage(hurt);
      }
      // A droideka doesn't drown: deep water wears it down (src/vehicle-water.js).
      if (water && unit.alive && !unit.seat && !unit.held && unit.walker) droidekaWater(unit.soldier, unit.combat, dt);
      // One a creature holds is no longer among the soldiers a region
      // tests (the 2004 game moves him to the riders' list). The region
      // kills (with an empty damage record) and deletes
      // nothing: the body lies where it falls as any other does, which is
      // what sets off Cloud City's carbon-freeze forklift at the bottom of
      // its pit (docs/live-objects.md "Animated props").
      if (unit.alive && !unit.held && inDeathRegion(unit.position)) unit.combat.damage(Infinity);
    }
    // Seats latch once per simulation step, after their controllers write
    // intent. This preserves trigger/exit edges at every physics rate.
    if (vehicles) {
      vehicles.spawnOccupant = combat.dead ? null : soldier.position;
      // His keys for the seat he holds now (src/boarding.js seatInput), as a
      // network human's are read for his.
      const seated = driver.vehicle;
      vehicles.update(dt, seated, seated ? seatInput(seated, seated.seatIndex, tickControls, command) : undefined);
      if (driver.active) driver.update(dt, tickControls);
    }
    turretSeats?.update(dt);
    if (gunner.active) gunner.update(dt, soldier, tickControls, 1);
    remotes.update(dt, remotes.active ? droidInput : null, tickControls);
    networkRemotes?.finish(tick, dt);
    soldier.sampleMuzzles?.(dt);
    if (!game.serverWorld) {
      combat.update(dt, tickControls, Math.max(Math.abs(input.forward), Math.abs(input.right)));
      combat.flushMuzzles();
    }
    const [low, high] = soldier.pitchRange();
    soldier.aimPitch = THREE.MathUtils.clamp(soldier.aimPitch, low, high);
    separateSoldiers();
    served.length = 0;
    if (!combat.dead && !driver?.active) served.push(supplied);
    for (const unit of factory?.built || []) if (unit.alive && !unit.seat && !unit.held) served.push(unit.supply);
    live?.update(dt, served, tick);
    ordnance.update(dt, false);
    bodyMiddle.set(soldier.position.x, soldier.position.y + BODY_MIDDLE, soldier.position.z);
    mineTargets.length = 0;
    if (!combat.dead && !gunner.active && !driver?.active) mineTargets.push(...itemTargets);
    for (const unit of factory?.built || []) if (unit.alive && !unit.seat && !unit.held) mineTargets.push(unit.itemTarget);
    if (vehicles) for (const v of vehicles.vehicles) if (v.alive) mineTargets.push(v.itemTarget);
    items.update(dt, mineTargets);
    if (water && !combat.dead && !gunner.active && !driver?.active && playerUnit.walker) droidekaWater(soldier, combat, dt);
    else if (water && !combat.dead && !gunner.active && !driver?.active) {
      const hurt = water.soldierStep(dt, waterContact, soldier.position, combat.pivot, combat.origin, soldier.velocity,
        at => presentEffect("watersplash_md", at));
      combat.gunUnderwater = Boolean(waterContact.gunUnder);
      if (hurt) combat.damage(hurt);
    }
    // Ten seconds outside the battlefield kills (src/battlefield.js).
    battlefield.update(dt, battle.characters);
    updateBattle(dt);
    updateTargetHud(dt);
    // Each live grenade warns the soldiers near it, after its first touch
    // and the difficulty's delay (the Classic AI's danger notifier).
    if (ai?.grenadeUpdate) {
      for (const g of ordnance.grenades) if (!g.gone) ai.grenadeUpdate(g, g.owner === "player" ? playerUnit : g.owner);
    }
    // The spawn screen hears what this tick did to the player.
    humanSpawn.flush();
  }

  // Soldiers push each other apart, as the original game's collision pass does
  // for soldiers, droids, and droidekas within 60 m of the camera: each
  // body's main_body is an upright cylinder for its posture, and an overlap
  // moves each along the contact by the other's share of their volumes.
  // Only side contacts are resolved here; one soldier standing on another's
  // head is not (provisional). Positions move; the next collision pass
  // keeps them out of the walls.
  const MAIN_BODY = { stand: [0.57, 1.65, 0.9], crouch: [0.45, 1.45, 0.65], prone: [0.8, 0.45, 0.25] };
  // A multiplayer world has no single simulation camera to scope the push
  // to (each player's view is his own): createMultiplayerWorld sets this so
  // the server separates every pair, everywhere, deterministically.
  const separationScope = { global: false };
  const pushers = [];
  function separateSoldiers() {
    pushers.length = 0;
    for (const unit of units.all()) {
      if (unit.seat || unit.held || !unit.soldier || unit.removed || !(unit.alive || unit.dead)) continue;
      if (unit === playerUnit && (driver?.active || gunner.active || combat.dead)) continue;
      if (!separationScope.global && unit.position.distanceToSquared(simulationCamera.position) > 60 * 60) continue;
      const [r, h, y] = unit.walker ? [1, 2, 1] : MAIN_BODY[unit.hitPosture] || MAIN_BODY.stand;
      pushers.push({ unit, r, bottom: unit.position.y + y - h / 2, top: unit.position.y + y + h / 2, volume: Math.PI * r * r * h });
    }
    for (let i = 0; i < pushers.length; i++) {
      for (let k = i + 1; k < pushers.length; k++) {
        const a = pushers[i];
        const b = pushers[k];
        if (a.top <= b.bottom || b.top <= a.bottom) continue;
        const pa = a.unit.position;
        const pb = b.unit.position;
        let dx = pa.x - pb.x;
        let dz = pa.z - pb.z;
        const reach = a.r + b.r;
        const d2 = dx * dx + dz * dz;
        if (d2 >= reach * reach) continue;
        let d = Math.sqrt(d2);
        if (d < 1e-4) {
          dx = Math.sin(a.unit.id);
          dz = Math.cos(a.unit.id);
          d = 1;
        }
        const depth = reach - Math.sqrt(d2);
        const share = depth / (a.volume + b.volume);
        pa.x += (dx / d) * share * b.volume;
        pa.z += (dz / d) * share * b.volume;
        pb.x -= (dx / d) * share * a.volume;
        pb.z -= (dz / d) * share * a.volume;
        a.unit.tickCount += 1;
        b.unit.tickCount += 1;
      }
    }
  }

  // The predicted own soldier takes the same crowd push the authority will
  // apply (separateSoldiers), against the page's copies of everyone else.
  // Without it every contact in a crowd is a correction 50 ms later. Only
  // the predicted body moves; the copies keep their authoritative places.
  function separatePredictedSoldier() {
    const body = (unit) => unit.walker ? [1, 2, 1] : MAIN_BODY[unit.hitPosture] || MAIN_BODY.stand;
    const [r, h, y] = body(playerUnit);
    const volume = Math.PI * r * r * h, bottom = soldier.position.y + y - h / 2, top = soldier.position.y + y + h / 2;
    for (const unit of units.all()) {
      if (unit === playerUnit || unit.seat || unit.held || !unit.soldier || unit.removed || !(unit.alive || unit.dead)) continue;
      const [or, oh, oy] = body(unit);
      const p = unit.position;
      if (top <= p.y + oy - oh / 2 || p.y + oy + oh / 2 <= bottom) continue;
      let dx = soldier.position.x - p.x, dz = soldier.position.z - p.z;
      const reach = r + or, d2 = dx * dx + dz * dz;
      if (d2 >= reach * reach) continue;
      let d = Math.sqrt(d2);
      if (d < 1e-4) { dx = Math.sin(playerUnit.id); dz = Math.cos(playerUnit.id); d = 1; }
      const otherVolume = Math.PI * or * or * oh;
      const share = ((reach - Math.sqrt(d2)) / (volume + otherVolume)) * otherVolume;
      soldier.position.x += (dx / d) * share;
      soldier.position.z += (dz / d) * share;
    }
  }

  function stepPlayer(dt) {
    input.jump = worldControls.take("jump");
    input.crouch = worldControls.take("crouch");
    input.prone = worldControls.take("prone");
    // A droideka folds and unfolds on the use key.
    input.use = walker && worldControls.take("use");
    // Dead, or flying a droid, the soldier takes no input.
    if (combat.dead || remotes.active) {
      input.forward = input.right = input.turn = 0;
      input.jump = input.crouch = input.prone = input.use = false;
    }
    // Manning a turret, the soldier stays at its seat.
    if (gunner.active || driver?.active) return;
    // With no body in the world (before his first spawn, or once his body is
    // gone) there is nothing to move.
    // Held by a creature (src/props.js), he goes where it takes him.
    if (playerUnit.removed || playerUnit.held) return;
    soldier.tick(input, dt);
    // Landing too hard hurts him as it does a bot (src/collision-damage.js).
    takeImpacts(playerUnit);
    // A death region kills him, as it does a bot; his body lies where it
    // falls for the usual 4 s, then goes, and he spawns again.
    if (!combat.dead && inDeathRegion(soldier.position)) combat.damage(Infinity);
  }

  // Command posts, the victory timer, and the HUD. A team that holds no post
  // while none are neutral has 20 s before it loses.
  const capturers = [];
  function updateBattle(dt) {
    // Who counts toward a post: living soldiers on foot whose class captures
    // (CapturePosts, 1 for soldiers and droidekas, 0 for heroes); never
    // anyone seated in a vehicle or turret.
    capturers.length = 0;
    for (const unit of units.all()) {
      if (!unit.soldier || !unit.alive || unit.seat || unit.held || (unit.classDef?.CapturePosts ?? 1) === 0) continue;
      if (unit === playerUnit && (driver?.active || gunner.active)) continue;
      capturers.push(unit);
    }
    posts.update(dt, capturers);
    for (const event of posts.events.splice(0)) {
      // The native award pass revisits the capture region's first 64
      // objects. It credits living Characters without repeating the
      // contribution team's, CapturePosts, or seated-state gates.
      if (event.kind === "captured") {
        let seen = 0;
        for (const unit of units.all()) {
          if (unit.removed || !event.post.contains(unit.position, unitCenter)) continue;
          if (++seen > 64) break;
          if (unit.alive) battle.point(unit.character);
        }
      }
      // A destructible post destroyed (src/objectives.js): a post point for
      // whoever hurt it last unless he is its owner's friend, and its
      // KillRegion goes off.
      if (event.destroyed) objectiveDestroyed(event);
      audio.on("post", event);
      ai?.postEvent?.(event);
      const name = event.post.desc.displayName || event.post.label;
      const team = event.kind === "captured" ? event.by : event.from;
      const key = event.kind === "captured" ? "game.infomsg.captured" : "game.infomsg.lost";
      hud.message(hud.text(key, battle.names[team] ?? "", name), posts.relation(team));
    }
    // The command vehicles' posts (src/mobile-posts.js): one whose vehicle
    // died is lost, and whoever hurt the vehicle last earns a post point
    // unless he is its owner's friend; one an enemy boarded
    // changes hands. The AI hears of either.
    for (const event of posts.mobileEvents()) {
      if (event.kind === "lost") {
        const attacker = event.vehicle?.lastAttacker === "player" ? playerUnit : event.vehicle?.lastAttacker;
        const destroyer = attacker?.character ?? null;
        if (destroyer) battle.postDestroyed(destroyer, units.relation(destroyer.team, event.from));
        announceLost(event);
      }
      ai?.postEvent?.(event);
    }
    ai?.beforeBattle?.(dt);
    battle.update(dt);
    ai?.afterBattle?.(dt);
    // Without the spawn screen, whenever the Classic spawn manager asks for
    // the player's choice while he is on his feet (the match's start), he
    // confirms his current post at once; after a death the same choice comes
    // from onRespawn. With the screen its choices are applied here, where the
    // manager makes its own (src/human-spawn.js).
    if (!game.serverWorld && policy.auto && ai?.humanReady && battle.human && !combat.dead) ai.humanReady(battle.human, { className: UNIT, post: spawnPosts()[spawnIndex] });
    if (!game.serverWorld) humanSpawn.update(dt, tickHuman);
    tickHuman = null;
    // His body lies where it fell for 4 s, then is gone, as a bot's is
    // (checked after the respawn that a URL's choice makes at that moment).
    if (combat.dead && !playerUnit.removed && combat.deathTime >= CORPSE_LINGER) removeCorpse();
    presentBattle(dt);
  }

  function objectiveDestroyed(event) {
    const building = event.post.building;
    const attacker = building?.lastAttacker === "player" ? playerUnit : building?.lastAttacker;
    const destroyer = attacker?.character ?? null;
    if (destroyer) battle.postDestroyed(destroyer, units.relation(destroyer.team, event.from));
    killRegion(event.post.desc.killRegion, {
      random: gameplayRandom,
      play: presentEffect,
      victims: () => [
        ...[...units.all()].filter((unit) => unit.soldier && unit.alive && !unit.removed).map((unit) => ({
          position: unit.position,
          kill: () => (unit === playerUnit ? combat.damage(Infinity) : unit.combat.damage(Infinity)),
        })),
        ...(vehicles?.vehicles || []).filter((v) => v.alive).map((v) => ({ position: v.position, kill: () => v.setHealth(0) })),
      ],
    });
  }

  // A command vehicle's post lost with it, announced as a post's loss is
  // (the original game's event 5): "EMPIRE LOST THE AT-AT".
  function announceLost(event) {
    hud.message(hud.text("game.infomsg.lost", battle.names[event.from] ?? "", event.post.label), posts.relation(event.from));
  }

  // The HUD's picture of the battle after a tick: who is where, his score,
  // the map and the posts. The page showing the battle draws it once a frame
  // from the state it has, over the time the frame covers (clientFrame).
  const seatHeading = new THREE.Vector3();
  function presentBattle(dt, yaw = soldier.aimYaw) {
    // The minimap follows a flown droid.
    const flown = remotes.piloted;
    // Seated, it turns with his seat (a craft's pilot: the frame it flies
    // by, as drawn); it reaches farther as what he controls goes faster
    // (src/hud.js minimapRange).
    if (driver?.active && !flown) { driver.reticleDirection(seatHeading); yaw = Math.atan2(seatHeading.x, seatHeading.z); }
    const moving = flown?.velocity ?? driver?.vehicle?.velocity ?? soldier.velocity;
    const speed = moving ? Math.hypot(moving.x, moving.z) : 0;
    hud.setUnits(units, {
      viewer: playerUnit, controllable: flown || driver?.vehicle?.aiTarget || gunner.turret?.aiTarget || playerUnit,
      viewerTeam: playerTeam, relation: (a, b) => units.relation(a, b), locals: (team) => units.locals(team),
      easy: TIER === 1, tickets: (team) => battle.reinforcements[team] ?? Infinity,
    });
    const character = battle.human;
    hud.stats({ kills: character?.kills, teamkills: character?.teamKills, deaths: character?.deaths, captures: character?.captures });
    hud.update(dt, posts, flown ? { position: flown.position, yaw: flown.yaw, team: playerTeam, dead: combat.dead, speed } : { position: soldier.position, yaw, team: playerTeam, dead: combat.dead, speed }, battle);
    if (!client) presentLeaving();
  }
  // Outside the battlefield: his warning, and his side's line as he goes
  // out, from what he controls as this page shows him (src/battlefield.js):
  // after each tick here, once a frame on a page showing a world elsewhere.
  function presentLeaving() {
    if (!leavingWarning) return;
    const outside = !combat.dead && !playerUnit.removed && battlefield.controlsOutside(playerUnit, driver?.vehicle ?? gunner.turret ?? undefined, remotes.piloted ?? null);
    if (leavingWarning.update(worldClock.tick, outside)) audio.on("leaving");
    hud.setLeaving(leavingWarning.alpha);
  }

  const hudAim = new THREE.Vector3();
  const hudLosDirection = new THREE.Vector3(), hudLosHit = makeHit();
  function reticleLos(from, to, source, target) {
    hudLosDirection.subVectors(to, from);
    const length = hudLosDirection.length();
    if (length < 1e-6) return true;
    const ownBody = source?.vehicle?.body ?? source?.turret?.bodies?.built ?? null;
    const hit = collision.cast(from, hudLosDirection.divideScalar(length), length, hudLosHit, ownBody);
    return !hit || Boolean(target?.vehicle && hit.body?.owner === target.vehicle) || Boolean(target?.turret && hit.body?.owner === target.turret)
      || Boolean(target?.building && hit.body?.owner === target.building);
  }
  function updateTargetHud(dt) {
    const flown = remotes.piloted;
    const controlled = flown || driver?.vehicle?.aiTarget || gunner.turret?.aiTarget || playerUnit;
    // What a weapon's reticle is on: its kept lock, else what its cone
    // finds along his aim.
    const targetOf = (weapon) => {
      const kept = !combat.dead && weapon && lockedUnit(weapon, units);
      return kept ? { unit: kept } : !combat.dead && weapon ? findReticleTarget({
        origin: flown ? flown.eye : controlled === playerUnit ? combat.origin : simulationCamera.position,
        dir: flown ? flown.aimDir : controlled === playerUnit ? combat.aimDir : playerUnit.seat?.forward(hudAim) ?? controlled.forward(hudAim),
        units, weapon, viewer: controlled, relation: (a, b) => units.relation(a, b), los: reticleLos,
      }) : null;
    };
    const relationOf = (target) => (target ? units.relation(playerTeam, target.unit.team) : null);
    const weapon = flown?.arsenal.current(0) || targetingWeapon(driver?.vehicle) || gunner.turret?.weapon || combat.arsenal.current(0);
    const target = targetOf(weapon);
    humanReticleTarget = target?.unit ?? null;
    if (headless) return;
    // Seated, each channel's reticle takes the colour of what its own
    // weapon is on, and the enemy panel shows channel 0's (the 2004 game
    // targets each channel's weapon apart).
    const seat = driver?.active ? driver.vehicle : null;
    const first = seat ? (seat.current(0) === weapon ? target : targetOf(seat.current(0))) : target;
    const second = seat ? (seat.current(1) === weapon ? target : targetOf(seat.current(1))) : null;
    hud.setReticleTarget(relationOf(first), null, relationOf(second));
    hud.enemyHealth(relationOf(first) < 0 ? first.unit : null);
  }

  // Project against the camera actually drawn, every render frame. Lock
  // time comes from simulation (also in worker mode), never the HUD clock.
  function presentWeaponLock() {
    hud.aimCamera = camera;
    const weapon = hud.reticle?.weapon;
    const target = weapon && hasMissileLock(weapon.def) && lockedUnit(weapon, units);
    hud.lockMarker = null;
    if (!target || combat.dead || weapon.reloading || weapon.busy || weapon.wait > 0) return;
    target.aimPoint(hudAim);
    // Follow the target's displayed pose between simulation ticks too.
    if (target.vehicle?.root) hudAim.copy(target.vehicle.center).applyMatrix4(target.vehicle.root.matrixWorld);
    else if (target.presentation?.position) hudAim.add(target.presentation.position).sub(target.position);
    hudAim.project(camera);
    if (hudAim.z < -1 || hudAim.z > 1) return;
    const duration = weapon.def.LockTime > 0 ? weapon.def.LockTime : weapon.def.MaxPressedTime;
    hud.lockMarker = { x: (hudAim.x + 1) / 2, y: (1 - hudAim.y) / 2,
      progress: duration > 0 ? Math.min(1, weapon.lockElapsed / duration) : 0 };
  }

  // Every page joins an authoritative server: a worker locally, WebRTC online.
  // Entity records feed presentation; only the owned actor is predicted.
  let client = null;
  // Checks step the world worker in lockstep with this page's frames
  // (lockstepFrame, tests/battle-page.mjs) instead of its own clock, once
  // the worker is up (window.__bf is there before).
  const lockstep = { on: false, clock: new FixedClock() };
  let markClientReady;
  const clientReady = new Promise((resolve) => { markClientReady = resolve; });
  function restorePrediction() {
    client?.mountedWeapons?.restore();
    client?.weapons?.restore();
    if (client?.remotePrediction) {
      const {droid,state} = client.remotePrediction;
      restoreRemoteReplay(remotes,droid,state);client.remotePrediction=null;
    }
    if (!client?.predicted) return;
    if (client.savedDroideka) {
      restoreDroidekaState(soldier, client.savedDroideka);
      client.savedDroideka = null;
    } else restoreCompactSoldier(soldier, client.saved);
    client.predicted = false;
  }
  // His own soldier ahead of the worker: the acknowledged state, then every
  // command the worker hasn't taken yet, through the same movement rules as
  // stepPlayer (keys, jump, crouch, prone, mouse look, the pitch limits).
  // Restore authoritative fields before applying deltas or predicting again.
  // Corrections arrive with the worker's packets. On foot only.
  const predictInput = { forward: 0, right: 0, turn: 0, jump: false, crouch: false, prone: false, use: false };
  const predictKeys = new CommandControls();
  const predictedBefore = new THREE.Vector3();
  function predict() {
    const replayActions = client.host.entityInput.replayActions(true, true);
    const actions = commands => commands.map(command => ({...command, actions: replayActions.actions.get(command.sequence) || []}));
    const pending = predictionCommands(actions(client.pending), actions([...client.pendingActions, ...replayActions.shieldCommands]), 0, client.held);
    if (remotes.piloted && !combat.dead && !playerUnit.removed) {
      const droid=remotes.piloted,state=captureRemoteReplay(remotes,droid);
      const prediction=predictRemote(remotes,droid,pending,worldClock.tick,playerUnit.generation,
        client.heldGeneration===playerUnit.generation?client.held:[],client.replayView,client.remoteReplay);
      restoreRemoteReplay(remotes,droid,prediction.state);
      client.remotePrediction={droid,state};client.predictedTick=prediction.tick;
      client.predictionWindow.reset();client.vehicleWindow.reset();client.vehiclePrediction=null;
      return false;
    }
    client.remoteReplay.reset();
    const previousVehiclePrediction = client.vehiclePrediction;
    client.vehiclePrediction = null;
    // Entity protocol: a hull is replayed only once the server sends an exact
    // correction for it; other mounted bodies use the hull's records until then.
    const hullCorrected = client.entity?.previous?.vehicleId === driver?.vehicle?.aiTarget.id;
    if (driver?.active && driver.vehicle.seatIndex === 0 && !combat.dead && hullCorrected) {
      const context = { source: driver.vehicle, identity: playerUnit.id, epoch: commandHistory.epoch, generation: playerUnit.generation,
        authorityTick: worldClock.tick, pending, held: client.heldGeneration === playerUnit.generation ? client.held : [] };
      const continuation = client.vehicleWindow.select(context);
      client.vehiclePrediction = continuation.checkpoint && !continuation.commands.length ? continuation.checkpoint.state
        : predictVehicle(continuation.checkpoint?.state || driver.vehicle, continuation.commands, continuation.from, playerUnit.generation, continuation.held);
      if (client.vehiclePrediction) {
        const previous = previousVehiclePrediction;
        const current = client.vehiclePrediction;
        const atPrevious = previous && current.predictionPath.get(previous.predictionTick);
        if (!continuation.checkpoint) {
          if (previous?.predictionSource === driver.vehicle && atPrevious) {
            client.vehicleSmoother.note(...previous.position.toArray(), ...atPrevious.toArray());
          } else client.vehicleSmoother.reset();
        }
        current.predictionSource = driver.vehicle;
        client.vehicleWindow.save({ ...context, tick: current.predictionTick, commands: continuation.commands,
          held: continuation.held, state: current });
      }
    } else client.vehicleWindow.reset();
    if (playerUnit.seat && !combat.dead) {
      client.mountedWeapons ||= new MountedWeaponPrediction();
      client.mountedWeapons.predict(playerUnit, pending, worldClock.tick,
        client.heldGeneration === playerUnit.generation ? client.held : [], client.replayView);
    }
    if (combat.dead || playerUnit.removed || driver?.active || gunner.active || remotes.active || playerUnit.seat || playerUnit.held) {
      client.predictionWindow.reset();
      client.predictedTick = 0;
      return false;
    }
    if (walker) client.savedDroideka = captureDroidekaState(soldier);
    else client.saved = captureCompactSoldier(soldier);
    // The commands go on tick by tick as the worker's mailbox takes them
    // (reconcile.js replayTicks): keys stay held through a tick that has no
    // command of its own, and a command the worker will take late goes on
    // the first tick it can.
    const context = { source: soldier, identity: playerUnit.id, epoch: commandHistory.epoch, generation: playerUnit.generation,
      authorityTick: worldClock.tick, pending, held: client.heldGeneration === playerUnit.generation ? client.held : [] };
    const continuation = client.predictionWindow.select(context);
    if (continuation.checkpoint) {
      if (walker) restoreDroidekaState(soldier, continuation.checkpoint.state);
      else restoreCompactSoldier(soldier, continuation.checkpoint.state);
    }
    const { from, held: predictedHeld, commands } = continuation;
    client.weapons ||= new WeaponPrediction();
    client.weapons.begin(playerUnit,{epoch:commandHistory.epoch,tick:worldClock.tick,checkpoint:continuation.checkpoint?.weapons});
    const fireEdges=[...client.host.entityInput.outcomes.values()].map(item=>({...item.edge,sequence:item.sequence})).filter(edge=>edge.kind==='fire'&&edge.pressed);

    const infantryReplay = new (walker ? DroidekaPrediction : InfantryPrediction)(soldier, combat, continuation.checkpoint?.kick ?? combat.kick);
    const checkTick = !continuation.checkpoint && client.predictedTick > from ? client.predictedTick : 0;
    if (checkTick) predictedBefore.copy(client.predictedPosition);
    const last = replayTicks(commands, from, playerUnit.generation, predictedHeld, (tick, command, sequences) => {
      if (command.last) presentation.beforeStep();
      const yawBefore = soldier.aimYaw;
      const scale = command.mouseScale ?? settings.mouseScale, invert = command.invertMouse ?? settings.get("invertMouse");
      for (const [dx, dy] of command.look) (soldier.lookRaw || soldier.look).call(soldier, dx / combat.zoom, dy / combat.zoom, scale, invert);
      const lookYaw = soldier.aimYaw - yawBefore;
      predictKeys.down.clear();
      for (const key of command.down) predictKeys.down.add(key);
      predictInput.forward = predictKeys.axis("forward");
      predictInput.right = predictKeys.axis("right");
      predictInput.turn = predictKeys.axis("turn");
      predictInput.jump = command.actions.includes("jump");
      predictInput.crouch = command.actions.includes("crouch");
      predictInput.prone = command.actions.includes("prone");
      predictInput.use = command.actions.includes("use");
      infantryReplay.step(predictInput, 1 / 60, command.actions.includes("fireSecondary"));
      separatePredictedSoldier();
      // This tick's press on each trigger: its rounds draw the press's
      // seeded spread, as the authority's do (weapons.js spreadSeed).
      const presses=['firePrimary','fireSecondary'].map(name=>command.actions.includes(name)?predictedPress(fireEdges,sequences,name):null);
      client.weapons.step(command,tick,(shot,c,shotTick)=>{
        const button=shot.weapon.channel;
        const press=presses[button];
        if(press===false)return;
        const source=press?client.pending.find(command=>command.sequence===press.sequence):client.pending.find(command=>command.tick===shotTick);
        const traced={...c,sequence:source?.sequence??press?.sequence,epoch:commandHistory.epoch};
        game.predictedShots?.fire({id:press?.id||0,held:!press,button,pressed:true,tick:shotTick,epoch:commandHistory.epoch,generation:playerUnit.generation,...combat.convergedAim(soldier.aimYaw,soldier.aimPitch)},playerUnit,traced,shot);
      },presses.map(press=>press?press.id:null));
      const [low, high] = soldier.pitchRange();
      soldier.aimPitch = THREE.MathUtils.clamp(soldier.aimPitch, low, high);
      // The tick shown last frame, predicted again over newer authoritative
      // state: any difference is a correction, blended away if small.
      if (tick === checkTick) client.smoother.note(predictedBefore.x, predictedBefore.y, predictedBefore.z, soldier.position.x, soldier.position.y, soldier.position.z);
      if (command.last) {
        presentation.previousYaw += lookYaw;
        presentation.afterStep();
      }
    }, client.replayView);
    {
      const state = walker ? captureDroidekaState(soldier) : captureCompactSoldier(soldier);
      client.predictionWindow.save({ ...context, tick: last, commands, held: predictedHeld, state });
      client.predictionWindow.checkpoint.kick = infantryReplay.kick;
      client.predictionWindow.checkpoint.weapons = client.weapons.checkpoint();
    }
    client.predictedTick = last;
    client.predictedPosition.copy(soldier.position);
    return true;
  }
  async function startClient() {
    // Any human may change class, and remote soldiers may arrive in any order.
    await factory.prepareSoldiers();
    await Promise.all([1,2].flatMap(team=>playableClasses(teams[team])).map(name=>bodies.prepare(name)));

    const { NetworkHost } = await import('./runtime/network-host.js');
    const { LocalTransport } = params.has('network') ? {} : await import('./runtime/local-transport.js');
    client = {
      seats: new EntitySeats(game),
      running: false, ready: false, batch: [],
      pendingActions: [], ackActions: 0,
      // Online, remote actors are drawn one 20 Hz publication (3 ticks) plus
      // a margin behind the newest packet; measured arrival gaps widen it.
      latest: 0, clock: 0, playout: new PlayoutClock(params.has("network") ? { minDelay: 4, maxDelay: 10 } : {}), applyMs: 0, pending: [], inputClock: new FixedClock(), saved: null, predicted: false, playerAlpha: 1,
      // Command stamps follow the worker's estimated clock (reconcile.js);
      // the last acknowledged keys start each replay; small corrections to
      // the predicted body blend away instead of snapping.
      lead: new InputLead(params.has("network") ? { maxLead: 24, extrapolate: true, recover: true } : {}), target: 0, held: [], heldGeneration: 0,
      // Online, lateness is measured from the server's input-ack (the tick a
      // command actually ran, against the stamp below), not from the 20 Hz
      // correction that later retires it.
      commandStamps: new Map(), leadAck: 0, leadAckTick: 0, leadDropped: 0, entityReceiptAt: 0,
      predictionWindow: new PredictionWindow(), remoteReplay: new RemotePredictionContext(), vehicleWindow: new PredictionWindow(), continuationState: new Map(),
      vehicleSmoother: new CorrectionSmoother({ snap: 3, tau: .05 }),
      smoother: new CorrectionSmoother({ snap: 2.5, tau: .05 }), predictedTick: 0, predictedPosition: new THREE.Vector3(),
      // The worker's own health, from its packets (simulation-worker.js).
      health: null, stepMs: 0, packetMs: 0, packetBytes: 0, healthMax: 0, statsTick: null, statsDropped: 0,
    };
    // His own rounds from the trigger, and the world's copies drawn once
    // (CombatPresentation, on either transport).
    game.predictedShots = new PredictedShots(scene, collision, undefined, ordnance);
    // A sniper round striking a soldier as this page shows him: the hit
    // marker, and a head shot's flourish, the moment it lands, as the 2004
    // client flew its own rounds and saw them hit at once. The server's
    // damage feedback for that hit is not shown again
    // (network-combat-feedback.js claimPredictedHit); it still decides the
    // damage.
    const predictedHits = new Map();
    game.predictedShots.onStruck = (shot, hit) => {
      if (!(shot.bolt?.def?.FadeOutTime > 0) || !hit.unit?.alive || hit.unit.combat?.dead) return;
      hud.hit(units.relation((game.playerUnit ?? playerUnit).team, hit.unit.team));
      predictedHits.set(hit.unit.id, performance.now());
      if (hit.part === "head") ordnance.presentHeadshot(hit.point);
    };
    game.claimPredictedHit = (targetId) => {
      const at = predictedHits.get(targetId);
      if (at === undefined) return false;
      predictedHits.delete(targetId);
      return performance.now() - at < 1500;
    };
    if(inputTrace){game.predictedShots.onPredicted=(edge,command,metadata)=>inputTrace.predicted(edge,command,metadata);game.predictedShots.onConfirmed=(metadata,context)=>inputTrace.confirmed(metadata,context);}
    const entityModules = await Promise.all([
      import('./runtime/network-entity-game.js'), import('./runtime/network-entity-registry.js'), import('./runtime/network-entity-input.js'),
    ]);
    client.host = new NetworkHost({
      ...(LocalTransport ? { transport: new LocalTransport() } : {}),
      onEntityRegistry: descriptor => {
        const [{EntityGameState},{createEntityRegistry,verifyEntityRegistry},{INPUT_CODES,DEFAULT_BINDING_PROFILES}] = entityModules;
        const registry = verifyEntityRegistry(createEntityRegistry(game.data, {contentEpoch:descriptor.contentEpoch,inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES,posts:entityPostNames(game)}), descriptor);
        client.entity = new EntityGameState(game, registry, {
          beforeApply: restorePrediction,
          onCorrection: (context, packet) => {
            if (context.reset) { resetPredictionIdentity(client); view.reset(); bodyLaid = null; }
            worldClock.tick = context.tick; simTime = context.tick / 60;
            client.held = context.held; client.heldGeneration = context.generation; client.replayView = context.view;
            const retiredInput=retireAuthoritativeInputs(client,{ack:packet.ackInput,discardedInput:context.discardedInput,heldInput:context.held,viewInput:context.view,generation:context.generation,tick:context.tick,measureLead:false});
            client.pending = client.pending.filter(command => command.generation === context.generation);
            client.host.entityInput.ack(retiredInput);
            client.host.entityInput.applied(context.tick);
            client.ackActions = 0;
            client.pendingActions = client.pendingActions.filter(command => command.generation === context.generation && (client.host.entityInput.pendingActions(command.sequence).length || client.host.entityInput.pendingHuman(command.sequence)));
            updateTargetHud(1 / 20);
            // His death and his next life as the local world shows them: the
            // death camera over the corpse, stopped once he has a body again.
            // A creature that seizes him with its class's flag turns his view
            // to it at once, and to the chase camera, out of first person
            // (the 2004 game's grab).
            const seized = playerUnit.held?.prop?.deathCamera && !playerUnit.removed;
            if (playerUnit.held && firstPerson) setPerspective(false);
            if ((combat.dead && !playerUnit.removed || seized) && !deathView.active) startDeathCamera();
            else if (!combat.dead && !playerUnit.held && deathView.active) deathView.stop();
          },
          onApplied: packet => {
            const now = client.entityReceiptAt || performance.now() / 1000;
            // A publication is up to ten datagrams with one tick. The clocks
            // measure arrival gaps once per tick, not once per datagram:
            // same-tick repeats rewrite the last arrival time and shrink the
            // next measured gap.
            if (packet.tick > client.latest) { client.latest = packet.tick; client.playout.arrive(packet.tick,now,packet.interval); client.lead.arrive(packet.tick,now,client.leadDropped); }
            if (packet.snapshotAck) client.host.setSnapshotAck(packet.snapshotAck);
          },
        });
        client.entity.initialize();
        return registry;
      },
      onEntities: (bytes, at) => { client.entityReceiptAt = at || 0; client.entity.receive(bytes); },
      onGameplayEvents: (records, tick) => { client.entityReceiptAt = performance.now() / 1000; client.entity.receiveGameplay(records, tick); },
      onEntityDiagnostics: actors => {
        client.entityDiagnostics = actors;
        if(actors.length && !client.entityLimitShown){client.entityLimitShown=true;hud.message?.('Multiplayer: this scene contains an unsupported actor.');}
      },
      onChat: message => chat?.receive(message),
      playerName: settings.get("playerName"),
      onFire: (edge, command) => { if(playerUnit.seat)game.predictedShots?.fire(edge, playerUnit, command); },
      getAim: command => {
        if (playerUnit.seat) {
          const direction = driver?.active ? driver.viewDirection(new THREE.Vector3()) : gunner.active ? gunner.viewDirection(new THREE.Vector3()) : playerUnit.seat.forward(new THREE.Vector3());
          return { aimYaw: Math.atan2(direction.x, direction.z), aimPitch: Math.asin(Math.max(-1, Math.min(1, direction.y))) };
        }
        const body = commandBodyAim(soldier, walker, lookPreview, command, combat.zoom, settings.mouseScale, settings.get("invertMouse"));
        return combat.convergedAim(body.aimYaw, body.aimPitch);
      },
      onInputAck: ack => {
        inputTrace?.ack(ack);
        // The entity packets already feed the arrival clocks once per tick;
        // feeding this ack too (same tick, a moment later) shrinks the next
        // measured gap. The ack's own job is lateness: the tick the acked
        // command actually ran against its stamp, which is what widens the
        // lead when commands keep landing late. A dropped-tick report still
        // goes through arrive(), which resets the estimator's window.
        if (ack.generation === playerUnit.generation && ack.ack > client.leadAck) {
          const stamp = client.commandStamps.get(ack.ack);
          if (stamp !== undefined) client.lead.acknowledge(Math.max(0, ack.ackTick - stamp), client.leadAckTick ? ack.serverTick-client.leadAckTick : 1);
          client.leadAckTick = ack.serverTick;
          client.leadAck = ack.ack;
          for (const sequence of client.commandStamps.keys()) if (sequence <= ack.ack) client.commandStamps.delete(sequence);
        }
        if (Number.isSafeInteger(ack.dropped) && ack.dropped !== client.leadDropped) {
          client.leadDropped = ack.dropped;
          client.lead.arrive(ack.serverTick, ack.receivedAt, ack.dropped);
        }
      },
      onError: (error) => { console.error("world worker failed", error); hud.message?.(`World worker failed: ${error.message}`); },
    });
    const query = new URLSearchParams(params);
    query.delete("worker");
    const welcome = await client.host.init({ query: `?${query}`, settings: settings.values });
    client.spectator = welcome.spectator === true;
    if (client.spectator) {
      controls.human.length = 0;
      game.spawnScreen?.close("spectating");
    }
    // The server assigns teams (its TEAMS: AUTO ASSIGN) or lets the screen offer the switch.
    humanSpawn.canSwitchTeams = welcome?.teamSelect !== false;
    if (params.has("network") && chat) chat.send = message => client.host.chat(message);
    // A setting the battle reads while it runs (friendly fire), changed in
    // the pause menu: the world worker's copy follows (online, the server's).
    settings.onChange((key, value) => { if (WORLD_SETTINGS.includes(key)) client.host.settings?.({ [key]: value }); });
    await client.host.call('start'); client.running = true;
    do { await client.host.waitForEntities(); client.host.drainEntities(); }
    while (!client.entity.previous && !welcome.spectator && !client.host.closed);
    if(client.host.closed)throw Error(client.host.statusText || "Battle initialization failed");
    client.ready = true;
    // A background tab can finish loading without ever getting a frame or
    // another visibility event. Apply its frozen state at readiness too.
    if (!driving || frozen || held || document.hidden) clientFrame(0);
    markClientReady();
  }
  // Each frame: his input to the worker, the worker's ticks onto this world.
  function clientFrame(dt) {
    if (!client.ready) return;
    const run = driving && !frozen && !held && !document.hidden;
    if (run !== client.running) {
      client.running = run;
      client.host.call(run ? "start" : "suspend").catch(() => {});
      if (!run) {
        restorePrediction();
        game.predictedShots?.clear();
        client.predictionWindow.reset(); client.vehicleWindow.reset(); client.remoteReplay.reset();
        client.pending.length = 0;
        client.pendingActions.length = 0;
        client.inputClock.rebase();
        client.playout.reset();
        client.lead.reset(); client.commandStamps.clear(); client.leadAck = 0; client.leadAckTick = 0; client.leadDropped = 0; client.smoother.reset(); client.predictedTick = 0;
        presentation.reset();
        client.playerAlpha = 1;
        previewYaw = previewPitch = 0;
      }
    }
    if (!run) return;
    // Entity packets that arrived since last frame go on here, inside the
    // frame, before the playout clock and prediction consume them.
    client.host.drainEntities?.();
    client.pending = expireMovement(client.pending, client.latest, playerUnit.generation);
    // His input, a command per 60 Hz tick on a clock of its own, stamped
    // for the tick the worker will be at when each arrives plus a margin
    // (reconcile.js InputLead), so each command is the input of exactly one
    // world tick and none is late. A frame's commands go in one message.
    const input = client.inputClock, batch = client.batch;
    batch.length = 0;
    client.target = client.lead.target(performance.now() / 1000, client.latest, (client.host.path?.rtt || 0) / 1000);
    advanceInputClock(input, client.target, dt, (h, tick) => {
      const command = controls.command(tick, commandHistory.epoch, playerUnit.generation);
      if (client.spectator) return;
      // Paused online, the battle plays on and he stands still: keys held
      // at CLICK TO RESUME go nowhere and a gamepad's stick is centred
      // until he is back (stillCommand).
      if (paused) stillCommand(command);
      command.viewTick = renderedViewTick(client.clock, client.latest);
      inputTrace?.command(command);
      {
        client.commandStamps.set(command.sequence, command.tick);
        while (client.commandStamps.size > 512) client.commandStamps.delete(client.commandStamps.keys().next().value);
      }
      if (discreteActions(command).length || command.human?.length) {
        // A spun mouse wheel outruns the server's results: the oldest gives
        // way (only its local replay stops) rather than ending the session.
        if (client.pendingActions.length >= 128) client.pendingActions.shift();
        client.pendingActions.push(command);
      }
      client.pending.push(command);
      client.pending = retainPredictionCommands(client.pending, tick, playerUnit.generation, commandHistory.epoch);
      batch.push(command);
    }, client.target + 7, { lock: 0.9 });
    if (batch.length) client.host.inputs(batch);
    batch.length = 0;
    client.smoother.decay(dt);
    client.vehicleSmoother.decay(dt);
    // The acknowledged soldier back before the worker's ticks go on.
    restorePrediction();
    // Play out ticks on a clock of its own (src/runtime/playout.js): behind
    // the newest that has arrived by a delay measured from how unevenly the
    // packets arrive, at wall speed trimmed by at most 10%, so bodies move
    // evenly however the packets bunch up. Far behind (a stall), it jumps.
    client.clock = client.playout.advance(dt);
    presentAlpha = 1;
    client.predicted = predict();
    client.playerAlpha = client.predicted ? (client.predictedTick > worldClock.tick ? input.alpha : 1) : presentAlpha;
    // The look he has made since, over the aim: predicted, or the
    // acknowledged one plus every command's look the worker hasn't taken.
    lookPreview.aimYaw = soldier.aimYaw; lookPreview.aimPitch = soldier.aimPitch;
    if (!combat.dead && !walker) {
      const scale = settings.mouseScale, invert = settings.get("invertMouse");
      if (!client.predicted) for (const { look } of client.pending) for (const [dx, dy] of look) lookPreview.lookRaw(dx / combat.zoom, dy / combat.zoom, scale, invert);
      for (const [dx, dy] of controls.looks) lookPreview.lookRaw(dx / combat.zoom, dy / combat.zoom, scale, invert);
    }
    previewYaw = lookPreview.aimYaw - soldier.aimYaw;
    previewPitch = lookPreview.aimPitch - soldier.aimPitch;
    // A manned turret's look ahead of the worker, as the soldier's (gunner.js).
    if (gunner.active) gunner.preview(client.pending, controls.looks, settings.mouseScale, settings.get("invertMouse"), client.pending.length ? input.alpha : presentAlpha);
    client.entity?.present(client.clock);
    // A command vehicle lost is announced from the hulls this page shows:
    // the server's post summaries carry only the placed posts.
    for (const event of posts.mobileEvents()) if (event.kind === "lost") announceLost(event);
    // The HUD (the minimap's heading, the radar) follows the look he sees,
    // not the aim the server has acknowledged.
    presentBattle(dt, soldier.aimYaw + previewYaw);
    presentLeaving();
  }

  // A frame for checks that must come out the same every run
  // (tests/battle-page.mjs): this page's frames step the world worker
  // instead of its own clock. The ticks the page's clock crosses run in the
  // worker at once, each with this page's command for it; every packet goes
  // on before the frame, which shows the battle between the last two ticks
  // at the clock's phase, as a world clock of the page's own would. Nothing
  // is predicted. The first call stops the worker's clock for good.
  async function lockstepFrame(dt = 1 / 60, draw = false) {
    if (params.has("network")) throw new Error("lockstep needs the world worker");
    await clientReady;
    if (!lockstep.on) {
      const { tick } = await client.host.call("lockstep");
      lockstep.on = true; client.running = false;
      restorePrediction();
      // His rounds are the world's here, drawn as they come.
      game.predictedShots?.clear(); game.predictedShots = null;
      client.predictionWindow.reset(); client.vehicleWindow.reset(); client.remoteReplay.reset();
      client.pending.length = 0; client.pendingActions.length = 0; client.vehiclePrediction = null;
      client.host.drainEntities();
      if (worldClock.tick !== tick) throw new Error(`lockstep: the page is at tick ${worldClock.tick}, the worker at ${tick}`);
      presentation.reset(); previewYaw = previewPitch = 0;
      lockstep.clock.reset(); lockstep.clock.tick = tick; lockstep.clock.rebase();
    }
    pageKeys();
    const ticks = [];
    lockstep.clock.advance(dt, (h, tick) => ticks.push(tick), false);
    // A tick at a time: each tick's command is made once the tick before it
    // is on the page, for his life as it is then, as the page's own clock
    // made it (stamped for the life before, the mailbox would drop the rest
    // of a frame's commands after a respawn inside it).
    let applied = 0;
    for (const tick of ticks) {
      const command = controls.command(tick, commandHistory.epoch, playerUnit.generation);
      client.host.inputs([command]);
      await client.host.call("advance", { ticks: 1 });
      client.host.drainEntities();
      client.clock = tick; client.entity.present(tick); applied++;
    }
    if (worldClock.tick !== lockstep.clock.tick) throw new Error(`lockstep: tick ${worldClock.tick} applied, ${lockstep.clock.tick} run`);
    if (applied) updateTargetHud(applied / 60);
    if (gunner.active) gunner.preview([], null, 0, false, lockstep.clock.alpha);
    presentBattle(dt, soldier.aimYaw);
    frame(dt, draw, dt, true, false);
    return worldClock.tick;
  }

  // The presses the page answers itself, taken before the frame's commands
  // are made (they would go to the world in a command otherwise).
  function pageKeys() {
    if (controls.take("perspective")) { setPerspective(!firstPerson); if (driver?.vehicle?.flying) settings.set('flightFirstPerson', firstPerson); }
    if (controls.take("map")) hud.toggleMap();
    if (controls.take("stats")) settings.set("showStats", !settings.get("showStats"));
  }

  // Rendering samples a fixed 60 Hz world. Catch-up work is bounded per task;
  // outstanding ticks remain visible as debt, rather than silently vanishing.
  let presentAlpha = 1;
  const heldScale = new THREE.Vector3();
  // The spawn screen's camera shot ({ position, quaternion }), or null.
  let viewShot = null;
  const unitView = { position: new THREE.Vector3(), frustum: new THREE.Frustum(), frame: 0, matrix: new THREE.Matrix4() };
  const flightDebug = !headless && new URLSearchParams(location.search).has('flightDebug') ? new FlightDebug(scene) : null;
  let clock = 0;
  // liveLoop: the display loop's frames (bounded catch-up); tools and replays pass false.
  function frame(dt, draw = true, wallDt = dt, present = true, liveLoop = false) {
    runtimeProfile?.begin(dt, wallDt);
    pageKeys();
    runtimeProfile?.phase("simulation");
    if (!headless) {
      if (lockstep.on) presentAlpha = client.playerAlpha = lockstep.clock.alpha;
      else if (client) clientFrame(wallDt);
      else presentAlpha = 1;
    } else if (!frozen && !held) {
      worldClock.advance(wallDt, (h, tick) => {
        presentation.beforeStep();
        step(h, tick);
        presentation.afterStep();
      }, liveLoop);
      presentAlpha = worldClock.alpha;
    }
    if (headless || !present) { runtimeProfile?.end(false); return; }
    runtimeProfile?.phase("presentation");

    // At a capped frame rate, skipped callbacks still advance the world but
    // must not consume visual smoothing time a second time.
    // His own body: predicted ahead of a worker's battle, on its own clock.
    const playerAlpha = client ? client.playerAlpha : presentAlpha;
    if (!frozen && !document.hidden) {
      presentation.update(dt, playerAlpha);
      // The correction still being blended away (reconcile.js): drawn, never
      // collided with.
      if (client?.predicted) { const c = client.smoother; visual.x += c.x; visual.y += c.y; visual.z += c.z; }
    }

    if (driver?.active) driver.preview(frozen ? null : client?.pending, frozen ? null : controls.looks,
      settings.mouseScale, settings.get("invertMouse"));

    // The animated props at the tick drawn, and whom the creatures among
    // them hold, before any body is drawn (src/props.js). Held, his own
    // body is the creature's: in his clip, his root on its node.
    live?.presentAnimated(frozen ? 0 : dt, client?.clock ?? worldClock.tick);
    const held = playerUnit.held?.frame ? playerUnit.held : null;
    if (held) visual.setFromMatrixPosition(held.frame);
    const renderedSoldier = presentation.state;
    actor.root.position.copy(visual);
    renderedSoldier.aimYaw = soldier.aimYaw - presentation.tickYaw * (1 - playerAlpha) + previewYaw;
    renderedSoldier.aimPitch = soldier.aimPitch + previewPitch;
    if (held) animator.gunner(animator.clips.get(held.grab.clip) ?? null, 0, 0, held.time);
    // At a turret, his body stands at its seat in the gunner's pose, as
    // every other page draws him and the world strikes him (Gunner.poseBody).
    else if (gunner?.active) gunner.poseBody(renderedSoldier, actor.root);
    animator.update(frozen ? 0 : dt, renderedSoldier);
    if (held) held.frame.decompose(actor.root.position, actor.root.quaternion, heldScale);
    unitView.frame += 1;
    unitView.position.copy(camera.position);
    unitView.frustum.setFromProjectionMatrix(unitView.matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    for (const unit of factory?.built || []) unit.present(frozen ? 0 : dt, presentAlpha, lightActor, effects, unitView);
    game.predictedShots?.update(client?.clock ?? null);
    if (!frozen) vehicles?.present?.(presentAlpha, dt);
    if (!frozen && client?.vehiclePrediction && driver?.vehicle) {
      client.vehiclePrediction.present(client.inputClock.alpha);
      const smoothing = client.vehicleSmoother;
      driver.vehicle.root.position.x += smoothing.x;
      driver.vehicle.root.position.y += smoothing.y;
      driver.vehicle.root.position.z += smoothing.z;
      driver.vehicle.root.updateMatrixWorld(true);
      driver.vehicle.visual.copy(driver.vehicle.root.position);
    }
    remotes.present(frozen ? 0 : dt, settings.get("destructionEffects") === "enhanced", firstPerson);
    if (driver?.active) driver.presentRider(frozen ? 0 : dt);
    vehicles?.presentRiders(frozen ? 0 : dt);
    vehicleEffects?.update(vehicles?.vehicles, frozen ? 0 : dt);
    vehicleEffects?.updateBuildings(live ? [...live.turrets, ...live.objectives] : null, frozen ? 0 : dt);
    updateJetFlame(frozen ? 0 : dt);
    // In first person the eye is where the aim puts it, moving as he runs,
    // dives, lies down, gets up, and lands.
    const ownEyes = combat.usesEyeView && !combat.dead && !gunner.active && !driver?.active && !remotes.active;
    if (ownEyes) combat.viewEye(visual, eyePoint, renderedSoldier);
    if (!frozen) view.motion.update(dt, renderedSoldier, ownEyes);
    // Dead, the page's death camera (src/death-shot.js), the drawn camera's
    // only writer, from the body and the killer as drawn this frame; it
    // doesn't shake. Alive, it watches the view it will take over.
    if (deathView.active) deathShot.frame(frozen ? 0 : dt, camera, deathView, visual, ribcage, settings.get("deathCamera"));
    else if (remotes.active) remotes.updateCamera(dt, firstPerson);
    else if (driver?.active) driver.updateCamera(frozen ? 0 : dt, firstPerson);
    else if (gunner.active) gunner.updateCamera(dt, 1, firstPerson);
    else view.update(dt, visual, renderedSoldier.aimYaw, renderedSoldier.aimPitch, soldier.air ? "stand" : soldier.posture, combat.zoom, ownEyes, ownEyes ? eyePoint : null);
    if (!deathView.active) deathShot.watch(camera, frozen ? 0 : dt);
    layBody();
    // The arms hang from the view before any shake, so they shake with the
    // world.
    camera.updateMatrixWorld();
    firstPersonView?.update(frozen ? 0 : dt, { active: ownEyes, camera, soldier: renderedSoldier, shown: combat.shown, zoomed: combat.zoom > 1 });
    if (driver?.active) combat.clearCharge();
    else combat.presentCharge(frozen ? 0 : dt);
    cameraShake?.apply(camera, frozen ? 0 : dt, deathView.active);
    // The spawn screen's camera shot, in place of his view while it is set.
    // Only what is drawn (and heard) moves; the world's camera stays.
    if (viewShot) {
      camera.position.copy(viewShot.position);
      camera.quaternion.copy(viewShot.quaternion);
    }
    camera.updateMatrixWorld();
    runtimeProfile?.phase("audio");
    // Paused, the audio's own clock still runs (the menu's sounds; the
    // game's fade out): audio.setPaused.
    audio.update(dt);
    runtimeProfile?.phase("world");
    if (!frozen) {
      ordnance.present(presentAlpha, client ? client.clock : null, client?.entity ? () => { client.entity.combat.present(client.clock); game.predictedShots?.draw(); } : game.predictedShots ? () => game.predictedShots.draw() : null);
      debris.update(dt);
      destruction?.update(dt, camera);
      bubble?.update(dt, soldier, { position: visual });
      droidekaTrail?.update(dt, soldier, { position: visual, dead: combat.dead });
      foliage?.update(dt, camera);
      lightning.update(dt, camera);
      water?.update(dt, camera);
      waterShore?.update(dt, camera);
      posts.present(dt, camera);
      // The droids posed from the tick's state (a worker's arrives as plain
      // fields), and the build holograms for a soldier with a repair weapon.
      live?.present(dt, { camera, canRepair: !combat.dead && combat.arsenal.weapons.some((w) => w.repair) });
      turretSeats?.present();
      weather?.update(dt, camera);
      snow?.update(dt, camera);
      atmosphere?.update(dt, camera);
      skyFlyers?.update(dt, camera);
      environment?.update(dt, camera, !combat.dead && !driver?.active && !gunner.active ? [soldier] : []);
      effects.update(dt);
      clock += dt;
    }
    // Which bots' bodies and guns the crowd's shadow mesh draws this frame.
    graphics.crowdShadows?.update();
    runtimeProfile?.phase("hud");
    // The map follows the pose just drawn, including prediction correction
    // smoothing; sensor eligibility was already evaluated on gameplay state.
    if (hud.player) {
      const flown = remotes.piloted;
      hud.player.position = flown ? flown.mesh?.position ?? flown.position : driver?.active ? driver.vehicle.root.position : visual;
      if (!flown && driver?.active) {
        driver.reticleDirection(seatHeading);
        hud.player.yaw = Math.atan2(seatHeading.x, seatHeading.z);
      } else if (!flown && !gunner.active) hud.player.yaw = renderedSoldier.aimYaw;
    }
    lights.update(frozen ? 0 : dt);
    // The HUD shows the seat this page knows he holds, read afresh every
    // frame (a craft's included: src/hud.js draws every seat's displays).
    combat.updateHud(driver?.vehicle || gunner.turret, renderedSoldier, driver?.active ? driver : gunner.active ? gunner : null);
    presentWeaponLock();
    flightDebug?.update(driver?.vehicle, driver?.cameraView?.lookAt);
    graphics.setVehicleMotion(driver?.active ? driver.speedEffect : 0, driver?.vehicle?.yawRate || 0);
    lightSoldier(visual);
    // His droideka gone up or his droid blown apart, as anyone's (Unit.goUp):
    // what his corrections still say of the lingering body isn't drawn.
    if (walker && goneUp(renderedSoldier)) playerUnit.goUp();
    const ownGone = playerUnit.presentGone(frozen ? 0 : dt);
    if (ownGone > 0) playerUnit.dissolve(ownGone, settings.get("destructionEffects") === "enhanced");
    if (actor.shadow) actor.shadow.visible = !ownGone;
    firstPersonView?.light(soldierLight);
    runtimeProfile?.phase("render");
    if (draw) {
      water?.reflect(camera);
      environment?.reflect(camera, water);
      graphics.render(clock);
      water?.drawUnderwater(graphics.renderer, camera);
      hud.render();
      graphics.profiler.frameEnd();
    }
    runtimeProfile?.end(draw);
    inputTrace?.submit(draw);
  }

  let fps = 0;
  let frameInterval = 1 / 60, statsAt = 0;
  const statsClock = {};
  let last = performance.now();
  let driving = true;
  const showStats = () => {
    if (stats) stats.hidden = !settings.get("showStats");
  };
  showStats();
  settings.onChange((key) => {
    if (key === "showStats") showStats();
  });
  const pollFence = () => graphics.profiler.poll();
  // One frame's failure must not end the animation loop (a frozen picture
  // with no error shown); it is reported, and the next frame still comes.
  let frameFailures = 0;
  function loop(now) {
    try { frameBody(now); } catch (error) {
      if (++frameFailures <= 3) console.error("frame failed", error);
      if (frameFailures === 1) hud.message?.(`Frame error: ${error.message}`);
    }
    requestAnimationFrame(loop);
  }
  // The worker client's stats: the worker's health from its packets (its
  // tick cost, its speed over the last quarter second, debt and dropped
  // ticks), then this page's side (apply cost, the playout delay, the input
  // lead, commands late by the page's count and the worker's, corrections).
  function clientStats() {
    const h = client.health, v = soldier.velocity;
    let worker = "worker not yet reporting";
    if (h) {
      const ran = worldClock.tick - (client.statsTick ?? worldClock.tick), dropped = h.dropped - client.statsDropped;
      client.statsTick = worldClock.tick; client.statsDropped = h.dropped;
      const speed = ran + dropped > 0 ? ran / (ran + dropped) : 1;
      worker = `worker ${h.tickMs.toFixed(1)} ms/tick (step ${client.stepMs.toFixed(1)}, packet ${client.packetMs.toFixed(2)} ms ${(client.packetBytes / 1024).toFixed(1)} KB ${client.host.shared ? "shared" : "posted"}, peak ${client.healthMax.toFixed(0)}), ${Math.round(speed * 100)}% speed, debt ${h.debt}, dropped ${h.dropped}, queued ${h.queued}`;
      client.healthMax = 0;
    }
    const c = client.smoother;
    return `${fps.toFixed(0)} fps  ${worker}  |  page ${client.applyMs.toFixed(1)} ms/tick, shown ${client.playout.delay.toFixed(1)} behind` +
      `${client.playout.waits ? `, waited ${client.playout.waits}` : ""}, lead ${client.lead.lead.toFixed(1)} (${(client.inputClock.elapsed * 60 - client.latest).toFixed(1)} sent), late ${client.lead.late} here / ${h?.late ?? 0} there` +
      `${c.count ? `, ${c.count} corrections ${c.worst.toFixed(2)} m max${c.snaps ? `, ${c.snaps} snapped` : ""}` : ""}  ${Math.hypot(v.x, v.z).toFixed(2)} m/s`;
  }
  function frameBody(now) {
    const wallDt = Math.max(0, (now - last) / 1000);
    const presentDt = framePacer.advance(wallDt);
    const dt = Math.min(presentDt ?? wallDt, 0.1);
    last = now;
    if (driving) {
      const started = performance.now(), profiler = graphics.profiler;
      // Every display refresh looks at the last frame's fence, drawn or not:
      // the sooner it is seen done, the more the GPU has to spare.
      profiler.poll(started);
      frame(dt, presentDt !== null && !veiled, wallDt, presentDt !== null, true);
      if (presentDt !== null && !frozen && !document.hidden) {
        graphics.setAdaptiveScale(resolution.sample(presentDt * 1000, performance.now() - started, profiler.gpu.at(-1), profiler.behind, profiler.latency));
        // Without a GPU timer, one more look halfway through the frame
        // budget: a fence done by then is headroom (the next refresh comes
        // a whole budget later when the cap is the display's rate).
        if (resolution.target && !profiler.extension) setTimeout(pollFence, 500 / resolution.target);
      }
    }
    if (presentDt !== null && stats && !stats.hidden) {
      // Report real display intervals, not the capped cosmetic dt. Update
      // the text at 4 Hz instead of invalidating its DOM on every refresh.
      frameInterval += (Math.max(presentDt, .001) - frameInterval) * (1 - Math.exp(-presentDt / .5));
      fps = 1 / frameInterval;
      if (now - statsAt >= 250) {
        statsAt = now;
        const v = soldier.velocity;
        // The world's speed over the last quarter second, below real time
        // when ticks had to be dropped.
        const ran = worldClock.tick - (statsClock.tick ?? worldClock.tick), dropped = worldClock.dropped - (statsClock.dropped ?? worldClock.dropped);
        statsClock.tick = worldClock.tick; statsClock.dropped = worldClock.dropped;
        const speed = ran + dropped > 0 ? ran / (ran + dropped) : 1;
        stats.textContent = (client
          ? clientStats()
          : `${fps.toFixed(0)} fps  60 Hz${speed < 0.995 ? ` (world at ${Math.round(speed * 100)}% speed, ${worldClock.stepCost.toFixed(1)} ms/tick)` : worldClock.debt ? ` (${worldClock.debt} ticks behind)` : ""}  ${Math.hypot(v.x, v.z).toFixed(2)} m/s` +
          `  ${soldier.air || (soldier.grounded ? soldier.posture : "airborne")}` +
          `  ${spawns[spawnIndex]?.[0] ?? ""}`) + `  ${graphics.statsText()}`;
      }
    }
  }
  // Until the GPU has done everything asked of it so far (a fence), looked
  // at a frame at a time: the loading screen draws through the same GPU
  // process thread as the battle's shader links and uploads, so the load
  // gives them to it a batch at a time (docs/shell.md). A hidden page has
  // no screen to keep smooth, nor frames to wait for.
  const nextFrame = () => new Promise((resolve) => { requestAnimationFrame(resolve); setTimeout(resolve, 100); });
  async function gpuIdle(cap = 2000) {
    if (document.hidden) return;
    const gl = renderer.getContext();
    const sync = gl.fenceSync?.(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
    gl.flush();
    const until = performance.now() + cap;
    do await nextFrame(); while (sync && gl.clientWaitSync(sync, 0, 0) === gl.TIMEOUT_EXPIRED && performance.now() < until);
    if (sync) gl.deleteSync(sync);
  }
  if (!headless) {
  // Compile every shader before the first frame (graphics.warmup), so new
  // sights never stall play on a compile; a few at a time.
  ordnance.prepare?.();
  const warm = await graphics.warmupPaced(gpuIdle);
  onProgress?.("shaders");
  if (runtimeProfile) console.info(`shader warmup: ${warm.programs} programs in ${warm.ms.toFixed(0)} ms (${warm.rest} at once)`);
  requestAnimationFrame(loop);
  document.addEventListener("visibilitychange", () => {
    controls.clear();
    // Hidden tabs may stop delivering animation frames altogether. Suspend
    // here so the local world cannot fill an undrained entity queue.
    if (client?.ready) clientFrame(0);
    last = performance.now();
    worldClock.rebase();
    framePacer.reset();
    resolution.reset();
  });

  window.addEventListener("resize", () => view.applyFov(camera.aspect));
  }

  // The spawn screen's way into the world (docs/human-spawn.md): plain
  // values to read, choices sent as world commands (so a worker or replay
  // makes them on the same tick), the drawn camera's shot, and the events
  // the screen opens and closes on. It exists in every host; without a
  // screen nothing calls it.
  // The common pack has the four factions' icons; the 2004 game showed its
  // local allies under the faction they fought with (the Naboo Guard,
  // Gungans and Wookiees with the Republic, the Geonosians with the CIS).
  const LOGOS = { rep: "com_icon_republic", cis: "com_icon_cis", imp: "com_icon_imperial", all: "com_icon_alliance",
    gar: "com_icon_republic", gun: "com_icon_republic", wok: "com_icon_republic", geo: "com_icon_cis" };
  const SIDE_OF_FACTION = { 1: "all", 2: "imp", 3: "rep", 4: "cis" };
  // A team's side: as map.json's spawnScreen has it, else from its
  // SetTeamName, else from its first class.
  function sideOfTeam(team) {
    const extracted = world.spawnScreen?.teams?.[team]?.side;
    if (extracted) return extracted;
    const named = SIDE_OF_FACTION[factionOf(teams[team]?.name)];
    if (named) return named;
    const first = playableClasses(teams[team])[0];
    return (first && factory.sideOf(first)) || null;
  }
  // Queued for the next command, at most one of each kind (a burst of
  // choices can't outgrow a command).
  function queueHuman(entry) {
    if (client?.spectator) return;
    if (!queueHumanCommand(controls.human, entry)) console.warn("spawn port: dropped an invalid choice", entry);
  }
  const sameShot = (a, b) => a === b || Boolean(a && b && a.position.equals(b.position) && a.quaternion.equals(b.quaternion));
  // Every roster is loaded now: keep teams 1 and 2's.
  rosters = new Map([1, 2].map((team) => [team, rosterOf(team)]));
  const spawnPort = {
    // { phase, team, className, classIndex, post, canSwitchTeams, over }
    humanState: () => ({ ...humanSpawn.state(), spectator: client?.spectator === true }),
    // { 1: { side, logo, objectives }, 2: ... }
    teams() {
      const out = {};
      for (const team of [1, 2]) {
        if (!teams[team]) continue;
        const sideName = sideOfTeam(team);
        out[team] = { side: sideName, logo: LOGOS[sideName] || null, objectives: [...(world.spawnScreen?.teams?.[team]?.objectives || [])] };
      }
      return out;
    },
    // [{ className, displayName, weapons }]: the team's classes, heroes
    // excepted, in mission order; weapons by slot. Names fall back to ids.
    roster(team) {
      const eligibility = client?.entity?.eligibility;
      return rosterOf(Number(team)).filter(className => !eligibility || eligibility.team !== Number(team) || eligibility.eligibleClasses.includes(client.entity.registry.classIds.get(className))).map((className) => {
        const def = sides[factory.sideOf(className)].units[className];
        const weapons = [...(def.weapons || [])].sort((a, b) => (a.slot ?? 0) - (b.slot ?? 0));
        return {
          className,
          displayName: def.displayName || className,
          weapons: weapons.map((w) => String(w.displayName ?? w.label ?? w.weapon ?? "").trim()).filter(Boolean),
        };
      });
    },
    // [{ name, displayName, position, owner, eligible, vehicles }] for a team.
    posts(team) {
      team = Number(team);
      return spawnChoices().map((post) => ({
        name: post.name,
        displayName: post.label,
        position: post.position.toArray(),
        owner: post.team || 0,
        eligible: client?.entity?.eligibility?.team === team ? client.entity.eligibility.eligiblePosts.includes(client.entity.registry.postIds.get(post.name)) : friendlySpawn(post, team, spawnRelation),
        vehicles: [...(world.spawnScreen?.postVehicles?.[post.name]?.[team] || [])],
      }));
    },
    // The post the screen selects for the team, kept or moved as the
    // original's check does; a post name or null.
    choosePost(team, previous = null) {
      const choices = spawnChoices();
      const named = choices.find((post) => post.name === previous) || null;
      return choosePost(choices, Number(team), named, spawnRelation)?.name ?? null;
    },
    // Spawn as className at the named post (null: the world's pick), on team
    // if given; applied on the next world tick, once he may spawn.
    request({ team = null, className, post = null } = {}) {
      if (team !== null && team !== undefined) queueHuman({ kind: "team", team: Number(team) });
      queueHuman({ kind: "spawn", className, post });
    },
    // The pause menu's RESPAWN: a normal death.
    suicide() {
      queueHuman({ kind: "suicide" });
    },
    // Withdraw a spawn choice not yet carried out (the screen leaving its
    // committed state, as the original's BACK).
    cancel() {
      queueHuman({ kind: "cancel" });
    },
    // A body of the class for the screen to draw, outside the world.
    previewBody: (className) => bodies.preview(className),
    // The mission's AddCameraShot views, in the world's frame.
    cameraShots: () => (world.spawnScreen?.cameraShots || []).map((shot) => ({ position: [...shot.position], quaternion: [...shot.quaternion] })),
    // A fresh copy of a model from the common hologram pack (the side logos
    // com_icon_republic, _cis, _imperial and _alliance; the shell_cube room),
    // for the screen to place, colour and fade: each part its own unlit,
    // transparent, double-sided material (added where the part adds), listed
    // in userData.materials; each mesh's texture scroll (MTRL, per second) in
    // its userData.scroll. Null headless, or for a name the pack lacks.
    commonModel(name) {
      return commonModel(holograms, name, commonTexture);
    },
    // Draw from this shot ({ position, quaternion }) every frame until it is
    // cleared with null. The world's own camera is never moved. A shot that
    // moves on from the last (cut false: a drifting camera) keeps the
    // temporal upscaler's history, as the player's own view does.
    setViewShot(shot, { cut = true } = {}) {
      const valid = Array.isArray(shot?.position) && shot.position.length === 3 && Array.isArray(shot?.quaternion) && shot.quaternion.length === 4 &&
        [...shot.position, ...shot.quaternion].every(Number.isFinite);
      const next = valid ? { position: new THREE.Vector3().fromArray(shot.position), quaternion: new THREE.Quaternion().fromArray(shot.quaternion).normalize() } : null;
      if ((cut || !viewShot || !next) && !sameShot(next, viewShot)) graphics.resetHistory?.();
      viewShot = next;
    },
    // "awaitingSpawn", "spawned", "teamChanged", "playerDied" -> unsubscribe
    on: (event, fn) => humanSpawn.on(event, fn),
  };

  // Hooks for scripted checks in a headless browser.
  const game = {
    // What every side, effect and vehicle definition was loaded from, for
    // naming definitions across threads (src/runtime/network-entity-game.js).
    // Each loaded side's effects under its own name (the registry filters
    // them to the mission's sides, network-entity-registry.js).
    data: { sides, effects: effectsDef, sideFx: Object.fromEntries([[SIDE, sideFx], ...Object.entries(otherSides).map(([name, other]) => [name, other.fx])].filter(([, fx]) => fx)), vehicles: vehicleDef, props: propsDef, world, common },
    presentEvent,
    // An explosion this page shows shakes its view (src/camera-shake.js).
    shakeExplosion,
    cameraShake,
    set presented(fn) { hooks.presented = fn; },
    get tickLookYaw() { return tickLookYaw; },
    get client() { return client; },
    // The death camera, for scripted checks of the death flow: the world's,
    // and the one this page draws.
    get deathView() { return deathView; },
    deathShot,
    noteAttacker,
    chat,
    runtimeProfile,
    inputTrace,
    worldClock,
    framePacer,
    resolution,
    commandHistory,
    worldControls,
    simulationCamera,
    view,
    deathView,
    get deathKiller() { return deathKiller; },
    THREE,
    ready,
    environment,
    scene,
    camera,
    renderer,
    graphics,
    settings,
    menu,
    // The body he plays now (adopt() changes it between lives).
    get soldier() { return soldier; },
    get presentation() { return presentation; },
    get animator() { return animator; },
    get actor() { return actor; },
    get clips() { return clips; },
    collision,
    posts,
    hud,
    battle,
    battlefield,
    leavingWarning,
    get combat() { return combat; },
    get firstPersonView() { return firstPersonView; },
    live,
    gunner,
    turretSeats,
    vehicles,
    driver,
    effects,
    ordnance,
    debris,
    destruction,
    items,
    remotes,
    createNetworkRemotes(capacity) {
      return networkRemotes ??= new NetworkRemoteWorld(remotes, {capacity, randomFor:key=>reconRandom(SEED,key)});
    },
    get networkRemotes() { return networkRemotes; },
    separationScope,
    lightning,
    foliage,
    water,
    lights,
    weather,
    snow,
    atmosphere,
    skyFlyers,
    controls,
    audio,
    waterShore,
    get bubble() { return bubble; },
    get droidekaTrail() { return droidekaTrail; },
    get jetpackEffects() { return jetpackEffects; },
    get jetpackNozzles() { return jetpackNozzles; },
    // The spawn screen's world side.
    spawnPort,
    humanSpawn,
    bodies,
    policy,
    adopt,
    setHumanTeam,
    applyAuthoritativeHumanTeam: applyNetworkHumanTeam,
    validateAuthoritativeHumanTeam: validateNetworkHumanTeam,
    get playerTeam() { return playerTeam; },
    keys: controls.down,
    // Scripted presses: pending.jump = true queues a jump.
    pending: new Proxy(
      {},
      {
        get: (_, name) => controls.queued.has(name),
        set: (_, name, value) => {
          if (value) controls.queued.add(name);
          else controls.queued.delete(name);
          return true;
        },
      },
    ),
    respawn,
    setPerspective,
    frame,
    lockstep: lockstepFrame,
    // A tick of the world, for whatever runs it (the world worker, a server,
    // a Node check). A page shows a battle and runs none.
    step(command) {
      if (!headless) throw new Error("the page runs no battle: step the world worker (game.lockstep)");
      if (command.tick !== worldClock.tick + 1) throw new Error("nonconsecutive world command");
      worldClock.advance(1 / 60, (h, tick) => {
        presentation.beforeStep(); step(h, tick, command); presentation.afterStep();
      });
    },
    units,
    simEvents,
    factory,
    playerUnit,
    randomState: () => ({ ai: random.seed, game: randomB?.seed ?? null, streams: Object.fromEntries([...streams.streams].map(([name, value]) => [name, value.seed])) }),
    get ai() {
      return ai;
    },
    pause() {
      driving = false;
      if (client?.ready) clientFrame(0);
      restorePrediction();
      worldClock.rebase();
    },
    resume() {
      driving = true;
      last = performance.now();
      framePacer.reset();
      resolution.reset();
    },
  };
  // The spawn screen (src/spawn-screen, docs/spawn-screen.md): unit, team
  // and post when the map loads and after every death, unless the URL
  // already names them. The pause menu's RESPAWN works either way.
  if (!headless) {
    const { createSpawnScreen, pauseRespawn } = await import("./spawn-screen/index.js");
    // Their views in the front end's look (src/menus/themes/page) unless the
    // URL asks for the original's menus.
    const screenView = params.get("menus") === "classic" ? null : "page";
    if (policy.screen) game.spawnScreen = createSpawnScreen({ port: spawnPort, hud, controls, menu, settings, common, view: screenView });
    else pauseRespawn({ menu, port: spawnPort, strings: common.strings });
    // The scoreboard (src/scoreboard, docs/scoreboard.md): Tab's table in
    // play, and the end-of-match table, whose DONE goes on to the next
    // battle of the playlist (src/launch.js; this one again without one).
    const { createScoreboard } = await import("./scoreboard/index.js");
    const { scoreboardPort } = await import("./scoreboard/port.js");
    game.scoreboardPort = scoreboardPort({ battle, teams: () => spawnPort.teams(), humanTeam: () => playerTeam,
      relation: (a, b) => units.relation(a, b), commonModel: (name) => spawnPort.commonModel(name),
      // A class, weapon or vehicle named by key alone (a record from a server).
      label: contentLabels(game.data),
      // DONE on the end table: the playlist's next battle, or online the
      // server's next one once it is up (NetworkHost.proceed).
      canRestart: () => !params.has("network") || client?.host?.rotationReady === true,
      restart: () => { if (params.has("network")) return client?.host?.proceed?.() === true; location.replace(nextUrl()); },
      network: params.has("network") });
    game.scoreboard = createScoreboard({ port: game.scoreboardPort, hud, controls, menu, settings, common, spawnScreen: game.spawnScreen, view: screenView });
    // Their own Esc opens the pause menu over them.
    menu.setEscapeOwners?.(game.spawnScreen, game.scoreboard, chat);
  }
  // Everything the first minutes will draw, on the GPU: every texture
  // requested has arrived (and no new request for a moment), each is
  // uploaded, and the shaders are compiled with them bound. The loading
  // screen waits for this, then begin() starts the battle.
  //
  // The uploads go a few milliseconds at a time with a frame between: the
  // GPU process serves them and the loading screen's own canvas in turn, so
  // one long batch holds that screen's frames (docs/shell.md). The shader
  // programs are left to link in the driver's own threads where the browser
  // compiles in parallel (graphics.js), and waited for here.
  let loadedOnce = null;
  game.loaded = () => (loadedOnce ??= (async () => {
    if (headless) return;
    const quiet = 400;
    const deadline = performance.now() + 60000;
    const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
    await ready;
    while (performance.now() < deadline && (texturesOut.size > 0 || performance.now() - texturesChanged < quiet)) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    const pending = new Set();
    const gather = (texture) => {
      if (texture?.isTexture && (texture.image || texture.mipmaps?.length)) pending.add(texture);
    };
    for (const texture of library?.cache.values() || []) gather(texture);
    for (const root of [graphics.scene, graphics.viewScene, graphics.farScene]) {
      root?.traverse((object) => {
        for (const material of [].concat(object.material || [])) {
          for (const value of Object.values(material)) if (value?.isTexture) gather(value);
          for (const uniform of Object.values(material.uniforms || {})) gather(uniform?.value);
        }
      });
    }
    // A batch of uploads at a time, the next only once the GPU has done the
    // last: a few milliseconds of the page's time queue far more of the GPU
    // process's, which the loading screen waits behind.
    const bytes = (texture) => texture.isCompressedTexture
      ? (texture.mipmaps || []).reduce((sum, level) => sum + (level.data?.byteLength || 0), 0)
      : (texture.image?.width || 0) * (texture.image?.height || 0) * (texture.generateMipmaps ? 5.4 : 4);
    const batch = 1 << 20;
    let queued = 0;
    for (const texture of pending) {
      renderer.initTexture(texture);
      queued += bytes(texture);
      if (queued >= batch) {
        await gpuIdle();
        queued = 0;
      }
    }
    graphics.warmup();
    const programs = () => renderer.info.programs || [];
    while (performance.now() < deadline && programs().some((program) => program.isReady && !program.isReady())) await frame();
    await frame();
    await frame();
  })());
  // What the load still waits on, for checks: texture URLs with requests out.
  game.texturesPending = () => [...texturesOut.keys()];
  // The loading screen's last two frames: the battle is drawn under them.
  game.unveil = () => { veiled = false; };
  // The loading screen is gone: the battle starts now, not while it showed.
  game.begin = () => {
    veiled = false;
    if (!held) return;
    held = false;
    worldClock.rebase();
    framePacer.reset();
    // The player's activity for friends (src/presence.js): the battle is
    // on, with the player's side and, online, the players in.
    if (!headless) followBattle({ spawnPort, scoreboardPort: { players: () => game.scoreboardPort?.players() ?? null } });
  };
  chatReady = true;
  if (!headless) window.__bf = game;
  if (!headless) await startClient();
  return game;
}
return start();
}

// Each side's hologram logo, as a factory making a fresh copy with its own
// material: color and opacity differ from post to post.
function hologramLogos(logos, models, texture) {
  const factories = new Map();
  for (const [sideName, model] of Object.entries(logos)) {
    const parts = models.get(model.toLowerCase());
    if (!parts) continue;
    factories.set(sideName, () => {
      const material = new THREE.MeshBasicMaterial({
        map: texture(parts[0].texture),
        transparent: true,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        side: THREE.DoubleSide,
      });
      const group = modelGroup(parts, () => material);
      group.userData.material = material;
      return group;
    });
  }
  return factories;
}
