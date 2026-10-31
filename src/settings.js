// Player settings, kept in localStorage. These are this port's own options;
// where one stands in for a setting of the 2004 game, the comment says so.
const KEY = "battlefront-eternal.settings";
// Keep existing players' preferences when upgrading from the original name.
const LEGACY_KEY = "battlefront-classic.settings";

export const DEFAULTS = {
  // Gameplay always advances at 60 Hz, independently of display refresh.
  physicsRate: "60",
  performancePreset: "custom",
  frameLimit: "0",
  dynamicResolution: "0",
  renderScale: 1,
  // How the 3D frame reaches the window when the render scale (or adaptive
  // resolution) shrinks it: rebuilt over several jittered frames
  // ("temporal"), in one pass ("spatial"), stretched by the texture filter
  // ("bilinear"), or by stretching the canvas ("off"). With an upscaler the
  // HUD stays at full resolution.
  upscaler: "temporal",
  upscaleSharpness: 0.4,
  // GPU-compressed textures (KTX2/Basis from the import, textures.js): a
  // quarter of the video memory and bandwidth. Read when a map loads.
  textureCompression: true,
  antialiasing: "4x",
  shadows: "high",
  anisotropy: 16,
  bumpMapping: true,
  // The map's soft-focus blur, as the game drew it.
  softFocus: true,
  // Optional enhancement, independent of the original map's blur.
  bloom: true,
  vehicleMotionBlur: true,
  softParticles: true,
  lampLighting: true,
  waterQuality: "high",
  treeDetail: "enhanced",
  droidekaEffects: "enhanced",
  // Enhanced: droids short out and blow apart into tumbling hot pieces, and
  // wrecked power-up droids smoulder (destruction.js); classic: the 2004
  // effects and pieces.
  destructionEffects: "enhanced",
  jetpackEffects: "enhanced",
  environmentEffects: "enhanced",
  rainEffects: "enhanced",
  oceanEffects: "enhanced",
  // Enhanced: lit plasma bolts in the gun's stock colour (blaster-bolts.js);
  // classic: the 2004 textured strips.
  boltEffects: "enhanced",
  // The arc caster's lightning (the HUD's DN BOLT CASTER). Enhanced: lit,
  // animated lightning that leaps from soldier to soldier and shocks the
  // ones it strikes (arc-lightning.js); classic: the 2004 textured ribbons.
  arcCasterEffects: "enhanced",
  // The sniper rifles' shots. Enhanced: a charged rail from the gun to the
  // impact with an ion trail, impact sparks and a head shot flourish
  // (sniper-trace.js); classic: the 2004 streak fading where it struck.
  sniperEffects: "enhanced",
  // Enhanced: sun-and-slope shading on ground baked without it (Hoth).
  terrainDetail: "enhanced",
  // The camera after a death (death-shot.js), drawn on this page only.
  // Enhanced: a replay of the kill, with the killing shot's trace, that
  // ends on the killer; classic: the 2004 game's framing and timing.
  deathCamera: "enhanced",
  flightSensitivity: 1,
  flightInvertY: false,
  flightResponseCurve: 1.35,
  flightFirstPerson: false,
  // The CONTROL page's keys the player changed: on foot and in vehicles
  // (src/input.js playerBindings) and in a starfighter (flightBindings).
  keyBindings: {},
  flightBindings: {},
  // "original" is the game's longest view distance (the map's near-scene
  // range); the others reach further.
  viewDistance: "original",
  // The far scene's terrain: every triangle ("full"), or a quarter of them
  // ("reduced", terrain-patches.js) for very weak GPUs.
  farTerrain: "full",
  // The PC options' mouse sensitivity slider: 0.5 to 10 in tenths, default 5.
  mouseSensitivity: 5,
  invertMouse: false,
  showStats: true,
  // PROFILE MANAGEMENT: the name the server and the other players see; the
  // original's first profile is PLAYER 1.
  playerName: "PLAYER 1",
  // The front end's look (src/menus/themes/page): "bf1", "bf2" or
  // "eternal", one layout in each game's colours.
  menuTheme: "eternal",
  // Options > Game > Friendly Fire, on by default as in the 2004 game: a hit
  // from one's own team does full damage (off: none, nor from one's own
  // blasts). Bots never hurt a human teammate either way.
  friendlyFire: true,
  aiMode: "off",
  aiDifficulty: "medium",
  aiHeroes: false,
  battleSize: "full",
  // Audio (src/audio): "enhanced" is the 2026 sound stage, "original" the
  // 2004 game's own output; the output layout ("auto" asks the device), the
  // dynamics profile, a mono fold, the enhanced mode's clarity stage, and the
  // volume sliders (0..1).
  audioQuality: "enhanced",
  audioOutput: "auto",
  audioDynamics: "normal",
  audioMono: false,
  audioClarity: true,
  masterVolume: 0.8,
  effectsVolume: 1,
  musicVolume: 1,
  voiceVolume: 1,
  ambienceVolume: 1,
  interfaceVolume: 1,
};

// Single-player battle size: the share of each mission's unit count that
// fights. Full is the 2004 game's. Smaller battles cost the CPU less on old
// PCs; the AI, classes and reinforcements are the same. (Online, the host's
// choice applies to everyone.)
export const BATTLE_SIZES = {
  full: { scale: 1, label: "Full (as the 2004 game)" },
  large: { scale: 0.75, label: "Reduced (3/4)" },
  small: { scale: 0.5, label: "Small (1/2, for old PCs)" },
};

export const PHYSICS_RATES = [["60", "60 Hz"]];

// The lowest render scale the options offer (a quarter of the window's
// width): weak GPUs at full screen draw a small frame and stretch it.
export const RENDER_SCALE_MIN = 0.25;

// The settings the battle itself reads while it runs, which the pause menu
// changes mid-battle (the rest reach it with each command, or at a restart).
// A world worker keeps its own copy in step with the page's (WorkerHost
// settings); online they are the server's.
export const WORLD_SETTINGS = Object.freeze(["friendlyFire"]);

// These only change presentation. Even the lightest preset keeps the same
// population, collision, input, AI and 60 Hz gameplay rules.
//
// Neither preset for old PCs uses the temporal or spatial upscaler: their
// passes run at the window's size whatever the render scale, and at full
// screen on an old GPU they cost more than the smaller frame saves
// (docs/performance.md, "Old GPUs at full screen").
export const PERFORMANCE_PRESETS = {
  // For integrated GPUs of about 2011 (Intel HD 2000/3000, AMD Kabini): half
  // resolution, stretched by the browser (no upscaler or antialiasing pass,
  // and no float targets), and the cheapest form of everything else.
  minimum: { label: "Minimum (very old PCs)", values: {
    renderScale: .5, upscaler: "off", antialiasing: "off", shadows: "off", anisotropy: 1,
    waterQuality: "low", treeDetail: "original", droidekaEffects: "classic",
    jetpackEffects: "classic", environmentEffects: "classic", rainEffects: "classic",
    oceanEffects: "classic", bumpMapping: false, softFocus: false, bloom: false,
    vehicleMotionBlur: false, softParticles: false, lampLighting: false,
    viewDistance: "original", dynamicResolution: "30", boltEffects: "classic", terrainDetail: "original",
    destructionEffects: "classic", farTerrain: "reduced", arcCasterEffects: "classic", sniperEffects: "classic",
  } },
  // Three-quarter resolution stretched by the bilinear upscaler (one read a
  // pixel, the HUD sharp over it), with FXAA.
  performance: { label: "Performance (older PCs)", values: {
    renderScale: .75, upscaler: "bilinear", antialiasing: "fxaa", shadows: "off", anisotropy: 2,
    waterQuality: "low", treeDetail: "original", droidekaEffects: "classic",
    jetpackEffects: "classic", environmentEffects: "classic", rainEffects: "classic",
    oceanEffects: "classic", bumpMapping: false, softFocus: false, bloom: false,
    vehicleMotionBlur: false, softParticles: false, lampLighting: false,
    viewDistance: "original", dynamicResolution: "30", boltEffects: "classic", terrainDetail: "original",
    destructionEffects: "classic", farTerrain: "full", arcCasterEffects: "classic", sniperEffects: "classic",
  } },
  balanced: { label: "Balanced", values: {
    renderScale: 1, upscaler: "temporal", antialiasing: "fxaa", shadows: "low", anisotropy: 8,
    waterQuality: "low", treeDetail: "original", droidekaEffects: "enhanced",
    jetpackEffects: "enhanced", environmentEffects: "classic", rainEffects: "classic",
    oceanEffects: "classic", bumpMapping: true, softFocus: true, bloom: true,
    vehicleMotionBlur: false, softParticles: true, lampLighting: true,
    viewDistance: "original", dynamicResolution: "60", boltEffects: "enhanced", terrainDetail: "enhanced",
    destructionEffects: "enhanced", farTerrain: "full", arcCasterEffects: "enhanced", sniperEffects: "enhanced",
  } },
  quality: { label: "High quality", values: Object.fromEntries([
    "renderScale", "upscaler", "antialiasing", "shadows", "anisotropy", "waterQuality", "treeDetail",
    "droidekaEffects", "jetpackEffects", "environmentEffects", "rainEffects", "oceanEffects",
    "bumpMapping", "softFocus", "bloom", "vehicleMotionBlur", "softParticles", "lampLighting",
    "viewDistance", "dynamicResolution", "boltEffects", "terrainDetail", "destructionEffects", "farTerrain",
    "arcCasterEffects", "sniperEffects",
  ].map(key => [key, DEFAULTS[key]])) },
};
const PRESET_KEYS = new Set(Object.keys(PERFORMANCE_PRESETS.performance.values));

// Multisampling followed by FXAA, so shader/specular edges are smoothed too.
export const ANTIALIASING = { off: 0, fxaa: 0, "2x": 2, "4x": 4, "8x": 8 };

export const WATER_QUALITIES = {
  low: { label: "Low (original)", scale: 0, maxSize: 512 },
  // High reflects the near scene in screen space (no second scene render);
  // Ultra renders it again from the mirrored camera at full resolution.
  high: { label: "High (screen-space reflections)", scale: 0.5, maxSize: 1024 },
  ultra: { label: "Ultra (full planar reflections)", scale: 1, maxSize: 2048, planarNear: true },
};

// How far the near scene (full detail) reaches, as a multiple of the map's
// own range; past it, the far scene's low-detail models take over.
export const VIEW_DISTANCES = {
  original: { scale: 1, label: "Original" },
  far: { scale: 2, label: "Far (2x)" },
  max: { scale: 4, label: "Maximum (4x)" },
};

// Cascaded shadow maps: map size per cascade, cascade count, and how far out
// shadows reach.
export const SHADOW_PRESETS = {
  off: null,
  low: { size: 1024, cascades: 2, far: 120 },
  medium: { size: 2048, cascades: 3, far: 200 },
  high: { size: 2048, cascades: 4, far: 300 },
  ultra: { size: 4096, cascades: 4, far: 450 },
};

// localStorage's settings: the current key's, else the original name's
// (copied to the current key).
function readLocal() {
  try {
    const current = localStorage.getItem(KEY);
    const legacy = current === null ? localStorage.getItem(LEGACY_KEY) : null;
    const stored = JSON.parse(current ?? legacy ?? "{}");
    if (legacy !== null) {
      try {
        localStorage.setItem(KEY, legacy);
      } catch {
        // Retain the loaded preferences even if storage is read-only/full.
      }
    }
    return stored && typeof stored === "object" ? stored : {};
  } catch {
    return {};
  }
}

export class Settings {
  // store: the desktop app's file of the settings (electron/player-settings.js,
  // bfeDesktop.settingsStore), which an update can't hide as it hides
  // localStorage; none in a browser, where localStorage is all there is.
  constructor(overrides = {}, { store = globalThis.bfeDesktop?.settingsStore ?? null } = {}) {
    let record = null, shadowed = false;
    if (store) {
      try {
        const read = store.read();
        record = read?.record && typeof read.record.values === "object" ? read.record : null;
        shadowed = !!read?.shadowed;
      } catch {
        store = null;
      }
    }
    // A complete file is the settings. Otherwise localStorage's, with what a
    // session that couldn't read them changed laid over them.
    const stored = record?.complete ? record.values : { ...readLocal(), ...(record?.values || {}) };
    this.store = store;
    // Whether this session knows every setting: false only while an update's
    // older version holds localStorage and no complete file exists yet. Then
    // only the player's own changes are written.
    this.complete = !store || !!record?.complete || !shadowed;
    this.changed = new Set(record && !record.complete ? Object.keys(record.values) : []);
    // What is saved: the stored settings and the player's own changes. The
    // overrides (the URL's, for this visit: ?ai=classic) apply on top and
    // are never saved, unless the player sets that setting himself.
    this.saved = { ...DEFAULTS, ...Object.fromEntries(Object.entries(stored).filter(([key]) => key in DEFAULTS)), physicsRate: "60" };
    this.values = { ...this.saved, ...overrides, physicsRate: "60" };
    this.listeners = new Set();
    // The first desktop session with the file: localStorage's settings move in.
    if (store && this.complete && !record?.complete) this.persist();
  }

  get(key) {
    return this.values[key];
  }

  set(key, value) {
    if (key === "physicsRate") value = "60";
    if (key === "performancePreset" && PERFORMANCE_PRESETS[value]) {
      for (const [name, setting] of Object.entries(PERFORMANCE_PRESETS[value].values)) this.set(name, setting);
    } else if (PRESET_KEYS.has(key) && this.values[key] !== value && this.values.performancePreset !== "custom") {
      this.set("performancePreset", "custom");
    }
    const changed = this.values[key] !== value;
    if (!changed && this.saved[key] === value) return;
    this.values[key] = value;
    this.saved[key] = value;
    this.changed.add(key);
    this.save();
    if (changed) for (const listener of this.listeners) listener(key, value);
  }

  reset() {
    for (const [key, value] of Object.entries(DEFAULTS)) this.set(key, value);
  }

  save() {
    try {
      localStorage.setItem(KEY, JSON.stringify(this.saved));
    } catch {
      // Private browsing can refuse storage; the settings still apply for now.
    }
    this.persist();
  }

  persist() {
    if (!this.store) return;
    const values = this.complete ? this.saved : Object.fromEntries([...this.changed].filter((key) => key in this.saved).map((key) => [key, this.saved[key]]));
    try {
      this.store.write({ complete: this.complete, values });
    } catch {
      // The desktop app gone mid-write (quitting): localStorage still has them.
    }
  }

  onChange(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  // The game's slider turns into a mouse multiplier of 2^((slider - 5) / 5).
  get mouseScale() {
    const slider = Math.min(Math.max(Number(this.values.mouseSensitivity) || 5, 0.5), 10);
    return 2 ** ((slider - 5) / 5);
  }
}
