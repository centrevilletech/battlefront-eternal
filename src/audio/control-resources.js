import { cycleHeights } from "../presentation/feet.js";
import { classSetting } from "../vehicle-motion.js";
import * as L from "./control-layout.js";
import { hashKey, keyOf } from "./hash.js";
import { normalizePcm16 } from "./pcm.js";

// Cold bank preparation assigns numeric identities to authored resources.
// These maps are never consulted by Rust's per-emitter control phase.
const SOUND_FIELDS = {
  PITCH: "Pitch", PITCH_DEV: "PitchDev", GAIN: "Gain", GAIN_DEV: "GainDev", REVERB: "ReverbGain", LOOP: "Looping", PAN: "Pan", MODE3D: "Mode3D", DOPPLER: "ModeDoppler", BIAS: "Bias", PRIORITY: "Priority",
  MIN_DISTANCE: "MinDistance", MUTE_DISTANCE: "MuteDistance", MAX_DISTANCE: "MaxDistance", ROLL_OFF: "RollOff", ROLL_IN_DISTANCE: "RollInDistance", PROBABILITY: "PlayProbability", INTERVAL: "PlayInterval", INTERVAL_DEV: "PlayIntervalDev", RANDOM_POSITION: "RandomPlayPos", CYCLE: "CyclePlayback",
};
const CLASS_FIELDS = {
  FIRE:"FireSound",RELOAD:"ReloadSound",OVERHEAT:"OverheatSound",CHARGE:"ChargeSound",FIRE_LOOP:"FireLoopSound",EMPTY:"FireEmptySound",CHANGE:"WeaponChangeSound",JUMP:"JumpSound",LAND:"LandSound",ROLL:"RollSound",SQUAT:"SquatSound",PRONE:"ProneSound",STAND:"StandSound",RUSTLE:"ClothingRustleSound",HURT:"HurtSound",DEATH:"DeathSound",ENGINE:"EngineSound",TURN_ON:"TurnOnSound",TURNING_OFF:"TurningOffSound",TURN_OFF:"TurnOffSound",TAKEOFF:"TakeoffSound",ACTIVATE:"TurretActivateSound",DEACTIVATE:"TurretDeactivateSound",YAW:"TurretYawSound",PITCH:"TurretPitchSound",IMPACT:"CollisionSound",BOUNCE:"CollisionOtherSound",SPLASH:"CollisionWaterSound",EXPLOSION:"SoundProperty",POWERUP:"PowerupSound",ORDNANCE:"OrdnanceSound",AMBIENT:"AmbientSound",AMBIENT2:"Ambient2Sound",TICK:"TickSound",HYDRAULIC:"HydraulicSound",STEP0:"FootstepSound0",STEP1:"FootstepSound1",STEP2:"FootstepSound2",STEP3:"FootstepSound3",STEP4:"FootstepSound4",STEP5:"FootstepSound5",DK_STEP0:"Footstep0Sound",DK_STEP1:"Footstep1Sound",DK_STEP2:"Footstep2Sound",WEAPON_LEFT:"0x1064c0a7",WEAPON_RIGHT:"0x10a991e4",COLLISION:"VehicleCollisionSound",DISCHARGE:"DischargeSound",DISPUTE:"DisputeSound",CAPTURE:"CapturedSound",LOST:"LostSound",
};
const CLASS_NUMBERS = { OVERHEAT_PITCH:"OverheatSoundPitch",CHARGE_PITCH:"ChargeSoundPitch",FIRE_STOP:"FireSoundStop",TICK_PITCH:"TickSoundPitch",TURN_OFF_TIME:"TurnOffTime",FOOTSTEP_STRIDE:"FootstepStride",YAW_PITCH:"TurretYawSoundPitch",PITCH_PITCH:"TurretPitchSoundPitch" };
const ALIASES = { TurningOffSound:["7d2c2fa2"],TakeoffSound:["WindUpSound","205b8d59"],LandSound:["WindDownSound","fc64a1f2"],HydraulicSound:["069134e5"],FootstepSound0:["97157af6"],FootstepSound1:["98157c89"],FootstepSound2:["951577d0"],FootstepSound3:["96157963"],FootstepSound4:["9b158142"],FootstepSound5:["9c1582d5"] };
const INPUT_NAMES = ["Speed","Acceleration","Health","TurnOnTime","TurnOffTime","WindUpTime","WindDownTime"].map(hashKey);
const SIDES = [null,"Rep","Cis","All","Imp"];
const number = (value) => value === undefined || value === null ? NaN : Number(value);
const first = (value) => Array.isArray(value) ? value[0] : value;
const ref = (value) => value ? String(first(value)).split(/\s+/)[0] : null;
const hash = (value) => Number.parseInt(keyOf(value).slice(2),16);
function row(layout, fill = NaN) { return new Float64Array(layout.STRIDE).fill(fill); }

export class ControlResources {
  constructor(data, renderer, game, enhanced) {
    this.data = data; this.enhanced = enhanced; this.renderer = renderer; this.game = game;
    this.tables = new Map(); this.soundIds = new Map(); this.paramIds = new Map(); this.classIds = new Map(); this.busIds = new Map(); this.sampleIds = new Map(); this.streamIds = new Map();
    this.sounds = []; this.soundObjects = new WeakMap(); this.streams = []; this.variants = []; this.segments = []; this.classRows = []; this.classNames = []; this.buffers = new Map(); this.materials = new Map([["terrain_foley",0],["water_foley",1]]);
    this.sides = {}; this.preparedFeet = new Set(); this.heightCount = 0;
    for (const [team, definition] of Object.entries(game.teams || {})) { const prefix = (definition.classes?.[0]?.[0] || "").split("_")[0].toLowerCase(); this.sides[team] = ({rep:"Rep",cis:"Cis",all:"All",imp:"Imp"})[prefix]; }
  }
  add(kind, value) { if (!this.tables.has(kind)) this.tables.set(kind, []); this.tables.get(kind).push(value); return value; }
  sound(value, parameterized = false) { const name = ref(value); if (!name) return -1; const key = keyOf(name); return (parameterized ? this.paramIds.get(key) : undefined) ?? this.soundIds.get(key) ?? -1; }
  class(name) { return this.classIds.get(name) ?? -1; }
  classValue(name, key) {
    const props = this.data.classSounds(name); if (!props) return null;
    let value = props[key]; if (!value) for (const alias of ALIASES[key] || []) { value = props[alias]; if (value) break; }
    return value;
  }
  material(name) { if (!this.materials.has(name)) this.materials.set(name, this.materials.size); return this.materials.get(name); }
  // Ports the cold parts of AudioEngine.setData and GameAudio.start. The
  // extracted files still arrive through the browser content loader, so
  // this serializer resolves their names to stable numeric resource rows.
  // Defaults, gates, score policy and mutable state belong to Rust.
  prepare() {
    const data = this.data, r = this.renderer;
    for (const [key, bus] of data.buses()) {
      const id = this.busIds.size; this.busIds.set(key,id); const name = row(L.NAME); name[L.NAME.KEY] = hash(key); name[L.NAME.SOUND] = id; this.add(L.IMPORT.BUS_NAME,name);
    }
    for (const [key, bus] of data.buses()) {
      const b = row(L.BUS); b[L.BUS.GAIN] = bus.Gain ?? 1; b[L.BUS.PARENT] = bus.Parent && bus.Parent !== key ? this.busIds.get(bus.Parent) ?? -1 : -1;
      b[L.BUS.TARGET] = bus.AttenuateBus ? this.busIds.get(bus.AttenuateBus) ?? -1 : -1; b[L.BUS.AMOUNT] = bus.Attenuation ?? 0.5; b[L.BUS.LEAD_IN] = bus.AttenuationLeadIn ?? 0.5; b[L.BUS.LEAD_OUT] = bus.AttenuationLeadOut ?? 0.5; this.add(L.IMPORT.BUS,b);
    }
    for (const id of Object.keys(data.samples)) {
      const sample = data.sample(id); const index = this.sampleIds.size; this.sampleIds.set(id,index); const s = row(L.SAMPLE,0);
      if (sample?.frames) {
        const rate = Math.max(sample.rate,8000); const buffer = r.ctx.createBuffer(1,sample.frames,rate); buffer.rateScale = sample.rate / rate;
        normalizePcm16(sample.pcm,buffer.getChannelData(0)); this.buffers.set(id,buffer);
        s[L.SAMPLE.RENDER_ID] = r.engineSample(buffer); s[L.SAMPLE.DURATION] = buffer.duration / (buffer.rateScale || 1); s[L.SAMPLE.RATE] = buffer.sampleRate; s[L.SAMPLE.SCALE] = buffer.rateScale || 1; s[L.SAMPLE.AVAILABLE] = 1;
      }
      this.add(L.IMPORT.SAMPLE,s);
    }
    for (const [name,index] of data.streams) {
      const id = this.streams.length; this.streamIds.set(hashKey(name),id); this.streams.push(index); const s = row(L.STREAM,0);
      const calls = (data.mission.streams || []).filter(([,stream]) => stream === name).length; s[L.STREAM.SLOTS] = calls || 1; s[L.STREAM.CHANNELS] = (index.channels || 1) * (index.substreams || 1); this.add(L.IMPORT.STREAM,s);
      index.controlSegments = new Map();
      for (const [key, entry] of Object.entries(index.segments)) {
        const segment = this.segments.length; index.controlSegments.set(key,segment); this.segments.push({index,key}); const record = row(L.SEGMENT);
        record[L.SEGMENT.STREAM] = id; record[L.SEGMENT.SAMPLE] = segment; record[L.SEGMENT.DURATION] = entry[2] / (index.substreams || 1) / entry[3]; record[L.SEGMENT.CHANNELS] = s[L.STREAM.CHANNELS]; const better = this.enhanced?.manifest.streams?.[index.name]?.[key]; const files = better && !better.fallback ? better.files || {} : {};
        const variants = new Map(); for (const [layout,file] of Object.entries(files)) { variants.set(layout,this.variants.length); this.variants.push({url:`${this.enhanced.base}/${file}`,playback:better.playback_db || 0}); }
        record[L.SEGMENT.STEREO] = variants.get("stereo") ?? -1; record[L.SEGMENT.SURROUND6] = variants.get("5.1") ?? -1; record[L.SEGMENT.SURROUND8] = variants.get("7.1") ?? -1; record[L.SEGMENT.FIRST] = variants.size ? variants.values().next().value : -1;
        this.add(L.IMPORT.SEGMENT,record);
      }
    }
    // SoundData has already resolved authored inheritance at load time.
    // Alias names share one object and therefore one native gate/cycle state.
    for (const kind of ["SoundProperties","SoundLayered","SoundParameterized","SoundStreamProperties","SoundStream"]) for (const [key,object] of data.byKind.get(kind) || []) {
      if (this.soundIds.has(key)) continue;
      let id = this.soundObjects.get(object); if (id === undefined) { id = this.sounds.length; this.sounds.push(object); this.soundObjects.set(object,id); }
      this.soundIds.set(key,id); const name = row(L.NAME); name[L.NAME.KEY] = hash(key); name[L.NAME.SOUND] = id; this.add(L.IMPORT.NAME,name);
    }
    for (const [key,object] of data.byKind.get("SoundParameterized") || []) {
      if (!data.object("SoundLayered",object.SoundLayered)) continue;
      let id = this.soundObjects.get(object); if (id === undefined) { id = this.sounds.length; this.sounds.push(object); this.soundObjects.set(object,id); } this.paramIds.set(key,id);
    }
    let entryCount = 0, bindingCount = 0, pointCount = 0;
    for (let id=0;id<this.sounds.length;id++) {
      const object = this.sounds[id], s = row(L.SOUND); const type = object.type;
      s[L.SOUND.KIND] = type === "SoundProperties" ? 1 : type === "SoundLayered" ? 2 : type === "SoundParameterized" ? 3 : 4;
      s[L.SOUND.BUS] = this.busIds.get(object.Bus || (s[L.SOUND.KIND] === 4 ? "0x9f9c4fd4" : "0xeceb308a")) ?? -1; s[L.SOUND.STREAM] = this.streamIds.get(keyOf(object.Stream)) ?? -1;
      for (const [field,key] of Object.entries(SOUND_FIELDS)) s[L.SOUND[field]] = number(object[key] ?? object[field === "RANDOM_POSITION" ? "3af79d76" : field === "CYCLE" ? "4d5af670" : ""]);
      s[L.SOUND.FIRST] = entryCount; const index = this.streams[s[L.SOUND.STREAM]];
      for (const entry of (s[L.SOUND.KIND] === 4 ? object.SegmentList : object.SampleList) || []) {
        const sample = s[L.SOUND.KIND] === 4 ? index?.controlSegments.get(entry[0]) : this.sampleIds.get(entry[0]); if (sample === undefined) continue;
        const e = row(L.ENTRY); e[L.ENTRY.ID] = sample; e[L.ENTRY.WEIGHT] = entry[1] ?? 1; e[L.ENTRY.INTERVAL] = entry[2] || 0; e[L.ENTRY.INTERVAL_DEV] = entry[3] || 0; this.add(L.IMPORT.ENTRY,e); entryCount++;
      }
      s[L.SOUND.COUNT] = entryCount - s[L.SOUND.FIRST]; this.add(L.IMPORT.SOUND,s);
      const layered = type === "SoundParameterized" ? data.object("SoundLayered",object.SoundLayered) : type === "SoundLayered" ? object : null;
      for (const [layerName,sound,delay=0,deviation=0] of layered?.Layer || []) {
        const layer = row(L.LAYER); layer[L.LAYER.PARENT] = id; layer[L.LAYER.SOUND] = this.sound(sound); layer[L.LAYER.DELAY] = delay; layer[L.LAYER.DEVIATION] = deviation; layer[L.LAYER.BINDING_FIRST] = bindingCount;
        if (type === "SoundParameterized") for (const child of object["Layer.children"] || []) {
          if (first(child.Name) !== layerName) continue; const [input,graph,output] = child.ParameterGraph || []; const b = row(L.BINDING); b[L.BINDING.INPUT] = INPUT_NAMES.indexOf(input); b[L.BINDING.OUTPUT] = output === hashKey("Gain") ? 0 : output === hashKey("Pitch") ? 1 : -1; b[L.BINDING.FIRST] = pointCount;
          for (const values of data.object("ParameterGraph",graph)?.ControlPoint || []) { const p = row(L.POINT); p[L.POINT.X] = values[0]; p[L.POINT.Y] = values[1]; this.add(L.IMPORT.POINT,p); pointCount++; }
          b[L.BINDING.COUNT] = pointCount - b[L.BINDING.FIRST]; this.add(L.IMPORT.BINDING,b); bindingCount++;
        }
        layer[L.LAYER.BINDING_COUNT] = bindingCount - layer[L.LAYER.BINDING_FIRST]; this.add(L.IMPORT.LAYER,layer);
      }
    }
    for (const name of Object.keys(data.classes)) {
      this.classIds.set(name,this.classRows.length); this.classNames.push(name); const props = data.classSounds(name); const record = row(L.CLASS,-1);
      for (const [field,key] of Object.entries(CLASS_FIELDS)) record[L.CLASS[field]] = this.sound(this.classValue(name,key), field === "ENGINE");
      for (const [field,key] of Object.entries(CLASS_NUMBERS)) record[L.CLASS[field]] = number(field === "TURN_OFF_TIME" || field === "FOOTSTEP_STRIDE" || field === "FIRE_STOP" ? props[key] : first(props[key]));
      for (let i=1;i<SIDES.length;i++) { const value = [].concat(props.AmbientSound || []).find((value) => String(value).split(/\s+/)[0].toLowerCase() === SIDES[i].toLowerCase()); record[L.CLASS["AMBIENT_"+SIDES[i].toUpperCase()]] = this.sound(value ? String(value).split(/\s+/).slice(1).join(" ") : null); }
      const stem = String(name).replace(/_[a-z0-9]+$/i,""); record[L.CLASS.TOW_ATTACH] = this.sound(stem+"_towcable_attach"); record[L.CLASS.TOW_RELEASE] = this.sound(stem+"_towcable_release");
      this.classRows.push(record); this.add(L.IMPORT.CLASS,record);
    }
    for (let id=0;id<this.classNames.length;id++) {
      const props = data.classSounds(this.classNames[id]); const properties = new Map();
      for (const [key,value] of Object.entries(props)) if (!/^[0-9a-f]{8}$/i.test(key)) properties.set(hash(key),this.sound(value));
      for (const [key,value] of Object.entries(props)) if (/^[0-9a-f]{8}$/i.test(key) && (properties.get(parseInt(key,16)) ?? -1) < 0) properties.set(parseInt(key,16),this.sound(value));
      for (const [key,sound] of properties) { const p = row(L.PROPERTY); p[L.PROPERTY.CLASS] = id; p[L.PROPERTY.KEY] = key; p[L.PROPERTY.SOUND] = sound; this.add(L.IMPORT.PROPERTY,p); }
    }
    for (const box of data.world.materials || []) { const m = row(L.MATERIAL); m.set(box.min,L.MATERIAL.MIN_X); m.set(box.max,L.MATERIAL.MAX_X); m[L.MATERIAL.GROUP] = this.material(box.group); this.add(L.IMPORT.MATERIAL,m); }
    for (const [material,materialId] of this.materials) for (let classId=0;classId<this.classNames.length;classId++) {
      const props = data.classSounds(this.classNames[classId]); const groupKey = hashKey(material), group = data.object("FoleyFXGroup",groupKey) || data.object("a1c9cbc5",groupKey); if (!group || !props.FoleyFXClass) continue;
      const fxClass = hashKey(props.FoleyFXClass); const entry = (group.FoleyFX || []).map(([name]) => data.object("FoleyFXSoldier",name)).find((entry) => entry?.Class === fxClass); if (!entry) continue;
      const fx = row(L.FOLEY,-1); fx[L.FOLEY.CLASS] = classId; fx[L.FOLEY.MATERIAL] = materialId;
      for (const [field,key] of Object.entries({LEFT:"FootstepLeftRun",RIGHT:"FootstepRightRun",JUMP:"jump",BODYFALL:"bodyfall",LAND:"Land",ROLL:"roll",SQUAT:"squat",PRONE:"prone",STAND:"stand"})) fx[L.FOLEY[field]] = this.sound(entry[key]); this.add(L.IMPORT.FOLEY,fx);
    }
    this.mission(); this.world();
    return this;
  }
  // Mission tables describe authored alternatives, not a current choice.
  // Native directors calculate ticket thresholds, stage priority, minimum
  // playback/idle intervals, bleed repetition and victory/defeat timing.
  mission() {
    const data = this.data, mission = data.mission, config = row(L.CONFIG,-1); const tracks = new Map();
    const track = (name) => {
      if (!name) return -1; if (tracks.has(name)) return tracks.get(name); const id = tracks.size; tracks.set(name,id); const object = data.object("Music",name); const t = row(L.TRACK);
      t[L.TRACK.STREAM] = this.sound(object?.SoundStream || object?.soundstream || name); t[L.TRACK.MIN_PLAY] = object?.MinPlaybackTime ?? object?.["6813be83"] ?? 0; t[L.TRACK.MIN_IDLE] = object?.MinInactiveTime ?? object?.["35724887"] ?? 0; t[L.TRACK.PRIORITY] = object?.Priority ?? 0; t[L.TRACK.FADE_IN] = object?.FadeInTime ?? 1; t[L.TRACK.FADE_OUT] = object?.FadeOutTime ?? 1; this.add(L.IMPORT.TRACK,t); return id;
    };
    for (const [team,list] of Object.entries(mission.music || {})) for (const [amount,name,stage,fraction] of list) {
      const s = row(L.MUSIC_SLOT); s[L.MUSIC_SLOT.TEAM] = Number(team); s[L.MUSIC_SLOT.STAGE] = stage; s[L.MUSIC_SLOT.TRACK] = track(name); s[L.MUSIC_SLOT.AMOUNT] = amount; s[L.MUSIC_SLOT.FRACTION] = fraction ?? 0; s[L.MUSIC_SLOT.START] = this.game.battle?.start?.[team] ?? 0; this.add(L.IMPORT.MUSIC_SLOT,s);
    }
    for (const [name,value] of Object.entries(mission.effects || {})) { const e = row(L.NAME); e[L.NAME.KEY] = hash(name); e[L.NAME.SOUND] = this.sound(value); this.add(L.IMPORT.EFFECT,e); }
    config[L.CONFIG.MASTER_BUS] = this.busIds.get("0xeceb308a") ?? -1; config[L.CONFIG.PAUSE_BUS] = this.busIds.get(hashKey("ingame")) ?? -1;
    config[L.CONFIG.OWN_SCOPE_IN] = this.sound(mission.effects?.ScopeDisplayZoomIn); config[L.CONFIG.OWN_SCOPE_OUT] = this.sound(mission.effects?.ScopeDisplayZoomOut);
    for (const team of [1,2]) { config[L.CONFIG["VICTORY"+team]] = track(mission.victory?.[team]); config[L.CONFIG["DEFEAT"+team]] = track(mission.defeat?.[team]); config[L.CONFIG["BOUNDS"+team]] = this.sound(mission.outOfBounds?.[team]);
      for (const actor of [1,2]) { const found = (mission.bleeding || []).find(([listener,bleeding,,normal]) => Number(listener) === team && Number(bleeding) === actor && Number(normal ?? 1) !== 0); config[L.CONFIG["BLEED"+team+actor]] = this.sound(found?.[2]); }
    }
    const spaces = [...(data.byKind.get("space")?.values() || [])], space = spaces.at(-1); const preset = space ? data.object("I3DL2ReverbPreset",space.I3DL2ReverbPreset) : null;
    config[L.CONFIG.SPACE_SEND] = space?.RoomGain ?? 0; const room = row(L.ROOM); room[L.ROOM.ROOM] = preset?.RoomGain ?? 0; room[L.ROOM.ROOM_HF] = Math.max(preset?.RoomHFGain ?? 1,0.001); room[L.ROOM.DECAY] = preset?.DecayTime ?? 1; room[L.ROOM.HF_RATIO] = preset?.DecayHFRatio ?? 0.5; room[L.ROOM.REFLECTIONS] = preset?.ERGain ?? 0; room[L.ROOM.REFL_DELAY] = preset?.ERDelay ?? 0.02; room[L.ROOM.REVERB] = preset?.ReverbGain ?? 0; room[L.ROOM.REVERB_DELAY] = preset?.ReverbDelay ?? 0.04; room[L.ROOM.DIFFUSION] = Math.min(Math.max(preset?.Diffusion ?? 1,0),1); room[L.ROOM.DENSITY] = 1; room[L.ROOM.HF_REFERENCE] = preset?.HFReference ?? 5000; room[L.ROOM.MASK] = preset ? 2047 : 1; this.add(L.IMPORT.ROOM,room);
    config[L.CONFIG.TOW_ATTACH] = this.sound("all_weap_fly_snowspeeder_towcable_attach"); config[L.CONFIG.TOW_RELEASE] = this.sound("all_weap_fly_snowspeeder_towcable_release");
    config[L.CONFIG.DEVICE_MEMORY] = globalThis.navigator?.deviceMemory ?? 8;
    config[L.CONFIG.SAMPLE_BASE] = this.renderer.nextSample; this.renderer.nextSample += 96; this.add(L.IMPORT.CONFIG,config);
  }
  // Region dimensions and scale remain authored facts. Rust computes their
  // reach and distance overrides, using the same hypot arithmetic as the
  // original Ambience constructor. Static emitters retain authored bounds.
  world() {
    for (const emitter of this.data.world.emitters || []) { const props = emitter.props || {}; const r = row(L.REGION); r[L.REGION.SOUND] = this.sound(props.Sound || props.SoundStream); r.set(emitter.position,L.REGION.X); r[L.REGION.MIN_DISTANCE] = Number(props.MinDistance ?? 1); r[L.REGION.MAX_DISTANCE] = Number(props.MaxDistance ?? 10); this.add(L.IMPORT.STATIC_EMITTER,r); }
    for (const region of this.data.world.regions || []) { const [kind,name,value] = region.name.split(/\s+/); if (kind.toLowerCase() !== "soundstream") continue; const r = row(L.REGION); const scale = Number(value); r[L.REGION.SOUND] = this.sound(name); r[L.REGION.X] = region.matrix[12]; r[L.REGION.Y] = region.matrix[13]; r[L.REGION.Z] = region.matrix[14]; r.set(region.size,L.REGION.SIZE_X); r[L.REGION.SCALE] = scale; this.add(L.IMPORT.REGION,r); }
    const water = this.game.water;
    if (water) { const w = row(L.WATER); w[L.WATER.CELL] = water.cell; w[L.WATER.GRID] = water.grid; w[L.WATER.OFFSET_X] = water.offset[0]; w[L.WATER.OFFSET_Z] = water.offset[1]; this.add(L.IMPORT.WATER,new Float64Array([...w,...water.cells])); }
    for (const plane of this.game.soundPlanes || []) this.add(L.IMPORT.PLANE,new Float64Array(plane));
  }
  // The existing rig loader supplies asset height samples at attachment.
  // This is separate from live pose capture: native feet.rs constructs the
  // schedule and reads every frame crossing, plant and primitive position.
  prepareFeet(v,host) {
    const cls = this.class(v.name); if (cls < 0 || this.preparedFeet.has(cls)) return;
    this.preparedFeet.add(cls); const feet = v.def?.fx?.feet || [], rules = v.def?.fx?.stomp || {};
    const cycle = feet.length && v.pose && v.walkLength > 0 && v.bones?.length;
    const heights = cycle ? cycleHeights(v,feet) : null, centred = !feet.some((foot)=>foot.bottom);
    for (let i=0;i<feet.length;i++) {
      const def = row(L.FOOT_DEF,0), point = feet[i].bottom || feet[i].at;
      def[L.FOOT_DEF.CLASS] = cls; def[L.FOOT_DEF.INDEX] = i; def.set(point,L.FOOT_DEF.X); def[L.FOOT_DEF.LENGTH] = cycle ? v.walkLength : 0;
      def[L.FOOT_DEF.DETECTION] = Number.isFinite(rules.detection) ? rules.detection : 0; def[L.FOOT_DEF.THRESHOLD] = Number.isFinite(rules.threshold) ? rules.threshold : 0.15; def[L.FOOT_DEF.HYDRAULIC] = Number.isFinite(rules.hydraulic) ? rules.hydraulic : 1;
      def[L.FOOT_DEF.CENTRED] = centred ? 1:0; def[L.FOOT_DEF.RATE] = parseFloat(classSetting(v.def,"AnimationSpeed")) || 1;
      def[L.FOOT_DEF.FIRST] = this.heightCount; def[L.FOOT_DEF.COUNT] = heights?.[i].length || 0;
      if (heights) { host.import(L.IMPORT.HEIGHT,heights[i]); this.heightCount += heights[i].length; }
      host.import(L.IMPORT.FOOT_DEF,def);
    }
  }
  upload(host) {
    // Dependency order is fixed at load: ranges refer only to tables already
    // imported. No generated file is merged or patched independently.
    for (const kind of [L.IMPORT.SAMPLE,L.IMPORT.BUS,L.IMPORT.ENTRY,L.IMPORT.SOUND,L.IMPORT.CLASS,L.IMPORT.POINT,L.IMPORT.BINDING,L.IMPORT.LAYER,L.IMPORT.STREAM,L.IMPORT.SEGMENT,L.IMPORT.TRACK,L.IMPORT.MUSIC_SLOT,L.IMPORT.REGION,L.IMPORT.STATIC_EMITTER,L.IMPORT.FOLEY,L.IMPORT.MATERIAL,L.IMPORT.WATER,L.IMPORT.PROPERTY,L.IMPORT.NAME,L.IMPORT.EFFECT,L.IMPORT.BUS_NAME,L.IMPORT.ROOM,L.IMPORT.CONFIG,L.IMPORT.PLANE]) {
      const rows = this.tables.get(kind); if (!rows?.length) continue; let length = 0; for (const row of rows) length += row.length; const values = new Float64Array(length); let at = 0; for (const row of rows) { values.set(row,at); at += row.length; } host.import(kind,values);
    }
  }
}
